use std::{
    cell::{Cell, RefCell},
    os::fd::OwnedFd,
    rc::Rc,
    time::{Duration, Instant},
};

use ashpd::{
    Error as PortalError,
    desktop::{
        CreateSessionOptions, PersistMode, ResponseError,
        screencast::{
            CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
            StartCastOptions,
        },
    },
};
use async_trait::async_trait;
use pipewire::{
    context::ContextRc,
    main_loop::MainLoopRc,
    properties::properties,
    spa::{
        param::{ParamType, format, video::VideoInfoRaw},
        pod::{Pod, Value, serialize::PodSerializer},
        utils::{Direction, Fraction, Rectangle, SpaTypes},
    },
    stream::{StreamFlags, StreamRc, StreamState},
};
use tokio_util::sync::CancellationToken;
use xcap::image::RgbaImage;

use super::backend::{
    CaptureBackend, CaptureCapabilities, CaptureError, CaptureErrorKind, CapturePermission,
    CaptureTarget, CaptureTargetKind,
};

const PORTAL_MONITOR_ID: &str = "portal:monitor";
const PORTAL_WINDOW_ID: &str = "portal:window";
const PIPEWIRE_TIMEOUT: Duration = Duration::from_secs(12);
const CANCELLATION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_FRAME_PIXELS: u64 = 50_000_000;

pub(crate) struct PortalCaptureBackend;

#[async_trait]
impl CaptureBackend for PortalCaptureBackend {
    async fn capabilities(&self) -> CaptureCapabilities {
        match Screencast::new().await {
            Ok(_) => CaptureCapabilities {
                targets: vec![CaptureTargetKind::Monitor, CaptureTargetKind::Window],
                permission: CapturePermission::UserPrompt,
                message: None,
            },
            Err(_) => CaptureCapabilities {
                targets: Vec::new(),
                permission: CapturePermission::Unknown,
                message: Some("This Wayland desktop does not provide screen sharing".to_owned()),
            },
        }
    }

    async fn targets(&self, kind: CaptureTargetKind) -> Result<Vec<CaptureTarget>, CaptureError> {
        let target = match kind {
            CaptureTargetKind::Monitor => CaptureTarget {
                id: PORTAL_MONITOR_ID.to_owned(),
                label: "Choose a screen…".to_owned(),
                kind,
            },
            CaptureTargetKind::Window => CaptureTarget {
                id: PORTAL_WINDOW_ID.to_owned(),
                label: "Choose a window…".to_owned(),
                kind,
            },
        };
        Ok(vec![target])
    }

    async fn capture(
        &self,
        target: &CaptureTarget,
        cancellation: CancellationToken,
    ) -> Result<RgbaImage, CaptureError> {
        let expected_id = match target.kind {
            CaptureTargetKind::Monitor => PORTAL_MONITOR_ID,
            CaptureTargetKind::Window => PORTAL_WINDOW_ID,
        };
        if target.id != expected_id {
            return Err(invalid_portal_target());
        }
        if cancellation.is_cancelled() {
            return Err(cancelled_error());
        }

        let portal = Screencast::new()
            .await
            .map_err(|error| map_portal_error(&error))?;
        let session = portal
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(|error| map_portal_error(&error))?;
        let result = tokio::select! {
            () = cancellation.cancelled() => Err(cancelled_error()),
            result = capture_portal_frame(&portal, &session, target.kind, cancellation.clone()) => result,
        };
        let _ = session.close().await;
        result
    }
}

async fn capture_portal_frame(
    portal: &Screencast,
    session: &ashpd::desktop::Session<Screencast>,
    kind: CaptureTargetKind,
    cancellation: CancellationToken,
) -> Result<RgbaImage, CaptureError> {
    let source = match kind {
        CaptureTargetKind::Monitor => SourceType::Monitor,
        CaptureTargetKind::Window => SourceType::Window,
    };
    portal
        .select_sources(
            session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Hidden)
                .set_sources(ashpd::enumflags2::BitFlags::from(source))
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|error| map_portal_error(&error))?
        .response()
        .map_err(|error| map_portal_error(&error))?;
    if cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    let streams = portal
        .start(session, None, StartCastOptions::default())
        .await
        .map_err(|error| map_portal_error(&error))?
        .response()
        .map_err(|error| map_portal_error(&error))?;
    if streams.streams().len() != 1 {
        return Err(CaptureError::new(
            CaptureErrorKind::Unavailable,
            "The screen sharing portal returned an unsupported source selection",
        ));
    }
    let stream_id = streams.streams()[0].pipe_wire_node_id();
    let remote_fd = portal
        .open_pipe_wire_remote(session, OpenPipeWireRemoteOptions::default())
        .await
        .map_err(|error| map_portal_error(&error))?;
    tokio::task::spawn_blocking(move || capture_pipewire_frame(remote_fd, stream_id, &cancellation))
        .await
        .map_err(|_| unavailable_error())?
}

#[allow(clippy::too_many_lines)]
fn capture_pipewire_frame(
    remote_fd: OwnedFd,
    stream_id: u32,
    cancellation: &CancellationToken,
) -> Result<RgbaImage, CaptureError> {
    let main_loop = MainLoopRc::new(None).map_err(|_| unavailable_error())?;
    let context = ContextRc::new(&main_loop, None).map_err(|_| unavailable_error())?;
    let core = context
        .connect_fd_rc(remote_fd, None)
        .map_err(|_| unavailable_error())?;
    let stream = StreamRc::new(
        core,
        "singularity-live-screen-frame",
        properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(|_| unavailable_error())?;
    let format = Rc::new(RefCell::new(None::<VideoInfoRaw>));
    let result = Rc::new(RefCell::new(None));
    let timed_out = Rc::new(Cell::new(false));
    let loop_for_timer = main_loop.clone();
    let result_for_timer = Rc::clone(&result);
    let timeout_for_timer = Rc::clone(&timed_out);
    let cancellation_for_timer = cancellation.clone();
    let started = Instant::now();
    let timer = main_loop.loop_().add_timer(move |_| {
        if cancellation_for_timer.is_cancelled() {
            *result_for_timer.borrow_mut() = Some(Err(cancelled_error()));
            loop_for_timer.quit();
        } else if started.elapsed() >= PIPEWIRE_TIMEOUT {
            timeout_for_timer.set(true);
            loop_for_timer.quit();
        }
    });
    timer
        .update_timer(
            Some(CANCELLATION_POLL_INTERVAL),
            Some(CANCELLATION_POLL_INTERVAL),
        )
        .into_sync_result()
        .map_err(|_| unavailable_error())?;

    let loop_for_state = main_loop.clone();
    let result_for_state = Rc::clone(&result);
    let loop_for_format = main_loop.clone();
    let result_for_format = Rc::clone(&result);
    let format_for_format = Rc::clone(&format);
    let loop_for_process = main_loop.clone();
    let format_for_process = Rc::clone(&format);
    let result_for_process = Rc::clone(&result);
    let cancellation_for_process = cancellation.clone();
    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(move |_, (), _, state| {
            if matches!(state, StreamState::Error(_)) {
                *result_for_state.borrow_mut() = Some(Err(unavailable_error()));
                loop_for_state.quit();
            }
        })
        .param_changed(move |_, (), id, param| {
            if id != ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else {
                return;
            };
            let mut video = VideoInfoRaw::default();
            if video.parse(param).is_err() {
                *result_for_format.borrow_mut() = Some(Err(unavailable_error()));
                loop_for_format.quit();
                return;
            }
            *format_for_format.borrow_mut() = Some(video);
        })
        .process(move |stream, ()| {
            if cancellation_for_process.is_cancelled() {
                *result_for_process.borrow_mut() = Some(Err(cancelled_error()));
                loop_for_process.quit();
                return;
            }
            let Some(video) = *format_for_process.borrow() else {
                return;
            };
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let offset = data.chunk().offset();
            let chunk_size = data.chunk().size();
            let stride = data.chunk().stride();
            let Some(bytes) = data.data() else {
                return;
            };
            let frame = rgba_from_buffer(bytes, offset, chunk_size, stride, video);
            *result_for_process.borrow_mut() = Some(frame);
            loop_for_process.quit();
        })
        .register()
        .map_err(|_| unavailable_error())?;

    let values = frame_format_values()?;
    let pod = Pod::from_bytes(&values).ok_or_else(unavailable_error)?;
    let mut param_refs = [pod];
    stream
        .connect(
            Direction::Input,
            Some(stream_id),
            StreamFlags::AUTOCONNECT | StreamFlags::MAP_BUFFERS,
            &mut param_refs,
        )
        .map_err(|_| unavailable_error())?;
    main_loop.run();
    if let Some(result) = result.borrow_mut().take() {
        return result;
    }
    if cancellation.is_cancelled() {
        return Err(cancelled_error());
    }
    if timed_out.get() {
        return Err(CaptureError::new(
            CaptureErrorKind::Unavailable,
            "The screen sharing frame timed out",
        ));
    }
    Err(unavailable_error())
}

fn frame_format_values() -> Result<Vec<u8>, CaptureError> {
    let object = pipewire::spa::pod::object!(
        SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        pipewire::spa::pod::property!(
            format::FormatProperties::MediaType,
            Id,
            format::MediaType::Video
        ),
        pipewire::spa::pod::property!(
            format::FormatProperties::MediaSubtype,
            Id,
            format::MediaSubtype::Raw
        ),
        pipewire::spa::pod::property!(
            format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            pipewire::spa::param::video::VideoFormat::BGRx,
            pipewire::spa::param::video::VideoFormat::BGRx,
            pipewire::spa::param::video::VideoFormat::RGBx,
            pipewire::spa::param::video::VideoFormat::BGRA,
            pipewire::spa::param::video::VideoFormat::RGBA
        ),
        pipewire::spa::pod::property!(
            format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            Rectangle {
                width: 1920,
                height: 1080
            },
            Rectangle {
                width: 1,
                height: 1
            },
            Rectangle {
                width: 8192,
                height: 8192
            }
        ),
        pipewire::spa::pod::property!(
            format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            Fraction { num: 1, denom: 1 },
            Fraction { num: 1, denom: 1 },
            Fraction { num: 60, denom: 1 }
        )
    );
    let values = PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &Value::Object(object))
        .map_err(|_| unavailable_error())?
        .0
        .into_inner();
    Ok(values)
}

fn rgba_from_buffer(
    bytes: &[u8],
    offset: u32,
    chunk_size: u32,
    stride: i32,
    video: VideoInfoRaw,
) -> Result<RgbaImage, CaptureError> {
    let size = video.size();
    let width = size.width;
    let height = size.height;
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_FRAME_PIXELS {
        return Err(unavailable_error());
    }
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(unavailable_error)?;
    let stride = usize::try_from(stride).map_err(|_| unavailable_error())?;
    if stride < row_bytes {
        return Err(unavailable_error());
    }
    let offset = usize::try_from(offset).map_err(|_| unavailable_error())?;
    let end = offset
        .checked_add(usize::try_from(chunk_size).map_err(|_| unavailable_error())?)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(unavailable_error)?;
    stride
        .checked_mul(usize::try_from(height).map_err(|_| unavailable_error())?)
        .and_then(|length| offset.checked_add(length))
        .filter(|required| *required <= end)
        .ok_or_else(unavailable_error)?;

    let mut rgba = Vec::with_capacity(
        usize::try_from(pixels)
            .ok()
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(unavailable_error)?,
    );
    let format = video.format();
    for y in 0..usize::try_from(height).map_err(|_| unavailable_error())? {
        let row_start = offset
            .checked_add(y.checked_mul(stride).ok_or_else(unavailable_error)?)
            .ok_or_else(unavailable_error)?;
        let (pixels, remainder) = bytes
            .get(row_start..row_start + row_bytes)
            .ok_or_else(unavailable_error)?
            .as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(unavailable_error());
        }
        for pixel in pixels {
            match format {
                pipewire::spa::param::video::VideoFormat::BGRx => {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
                }
                pipewire::spa::param::video::VideoFormat::BGRA => {
                    rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
                }
                pipewire::spa::param::video::VideoFormat::RGBx => {
                    rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
                }
                pipewire::spa::param::video::VideoFormat::RGBA => {
                    rgba.extend_from_slice(pixel);
                }
                _ => return Err(unavailable_error()),
            }
        }
    }
    RgbaImage::from_raw(width, height, rgba).ok_or_else(unavailable_error)
}

fn map_portal_error(error: &PortalError) -> CaptureError {
    match error {
        PortalError::PortalNotFound(_) => CaptureError::new(
            CaptureErrorKind::Unsupported,
            "This Wayland desktop does not provide screen sharing",
        ),
        PortalError::Response(ResponseError::Cancelled) => cancelled_error(),
        PortalError::Portal(ashpd::PortalError::NotAllowed(_)) => CaptureError::new(
            CaptureErrorKind::PermissionDenied,
            "Screen capture permission was denied",
        ),
        _ => unavailable_error(),
    }
}

const fn cancelled_error() -> CaptureError {
    CaptureError::new(CaptureErrorKind::Cancelled, "Screen capture was cancelled")
}

const fn invalid_portal_target() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::InvalidTarget,
        "The selected screen or window is unavailable",
    )
}

const fn unavailable_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::Unavailable,
        "The screen sharing service is unavailable",
    )
}

#[cfg(test)]
mod tests {
    use pipewire::spa::{
        param::video::{VideoFormat, VideoInfoRaw},
        utils::Rectangle,
    };

    use super::rgba_from_buffer;

    #[test]
    fn converts_one_padded_bgrx_pipewire_frame_to_rgba() {
        let mut video = VideoInfoRaw::new();
        video.set_format(VideoFormat::BGRx);
        video.set_size(Rectangle {
            width: 1,
            height: 2,
        });
        let bytes = [
            99, 99, 3, 2, 1, 0, 88, 88, 88, 88, 6, 5, 4, 0, 77, 77, 77, 77,
        ];

        let frame = rgba_from_buffer(&bytes, 2, 16, 8, video)
            .expect("a supported PipeWire frame is converted");

        assert_eq!(frame.dimensions(), (1, 2));
        assert_eq!(frame.as_raw(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn rejects_pipewire_frames_with_short_or_negative_stride() {
        let mut video = VideoInfoRaw::new();
        video.set_format(VideoFormat::RGBA);
        video.set_size(Rectangle {
            width: 1,
            height: 1,
        });

        assert!(rgba_from_buffer(&[1, 2, 3, 4], 0, 4, 3, video).is_err());
        assert!(rgba_from_buffer(&[1, 2, 3, 4], 0, 4, -4, video).is_err());
    }
}
