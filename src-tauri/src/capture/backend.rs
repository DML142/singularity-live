use std::{error::Error as StdError, fmt, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use xcap::{Monitor, Window, XCapError, image::RgbaImage};

use super::cursor::{
    CursorPositionProvider, MonitorResolver, PlatformCursorPositionProvider, XCapMonitorResolver,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTargetKind {
    Monitor,
    Window,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CaptureOperationId(Uuid);

impl CaptureOperationId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parses an operation identifier received from the desktop webview.
    ///
    /// # Errors
    ///
    /// Returns an invalid-target error when the value is not a UUID.
    pub fn parse(value: &str) -> Result<Self, CaptureError> {
        Uuid::parse_str(value).map(Self).map_err(|_| {
            CaptureError::new(
                CaptureErrorKind::InvalidTarget,
                "The capture operation identifier is invalid",
            )
        })
    }
}

impl Default for CaptureOperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureTarget {
    pub id: String,
    pub label: String,
    pub kind: CaptureTargetKind,
}

impl fmt::Debug for CaptureTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CaptureTarget")
            .field("id", &"[OPAQUE]")
            .field("label", &"[REDACTED]")
            .field("kind", &self.kind)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePermission {
    NotRequired,
    UserPrompt,
    Unknown,
    Denied,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureCapabilities {
    pub targets: Vec<CaptureTargetKind>,
    pub permission: CapturePermission,
    pub message: Option<String>,
}

impl CaptureCapabilities {
    #[must_use]
    pub fn is_supported(&self) -> bool {
        !self.targets.is_empty()
    }

    #[must_use]
    pub fn supports(&self, kind: CaptureTargetKind) -> bool {
        self.targets.contains(&kind)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureErrorKind {
    Unsupported,
    PermissionRequired,
    PermissionDenied,
    InvalidTarget,
    InvalidRegion,
    ImageExpired,
    Preparation,
    Cancelled,
    Busy,
    NoMatchingCapture,
    Unavailable,
}

#[derive(Clone, Eq, PartialEq)]
pub struct CaptureError {
    pub kind: CaptureErrorKind,
    pub message: &'static str,
}

impl CaptureError {
    #[must_use]
    pub const fn new(kind: CaptureErrorKind, message: &'static str) -> Self {
        Self { kind, message }
    }
}

impl fmt::Debug for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CaptureError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .finish()
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl StdError for CaptureError {}

#[async_trait]
pub trait CaptureBackend: Send + Sync {
    async fn capabilities(&self) -> CaptureCapabilities;

    async fn targets(&self, kind: CaptureTargetKind) -> Result<Vec<CaptureTarget>, CaptureError>;

    async fn capture(
        &self,
        target: &CaptureTarget,
        cancellation: CancellationToken,
    ) -> Result<RgbaImage, CaptureError>;

    async fn monitor_under_cursor(&self) -> Result<CaptureTarget, CaptureError> {
        let mut targets = self.targets(CaptureTargetKind::Monitor).await?;
        if targets.len() == 1 {
            return targets.pop().ok_or_else(unavailable_error);
        }
        Err(unavailable_error())
    }

    async fn target_by_id(&self, id: &str) -> Result<CaptureTarget, CaptureError> {
        for kind in [CaptureTargetKind::Monitor, CaptureTargetKind::Window] {
            if let Some(target) = self
                .targets(kind)
                .await?
                .into_iter()
                .find(|target| target.id == id)
            {
                return Ok(target);
            }
        }
        Err(CaptureError::new(
            CaptureErrorKind::InvalidTarget,
            "The selected screen or window is unavailable",
        ))
    }
}

#[derive(Default)]
pub struct XCapCaptureBackend;

#[async_trait]
impl CaptureBackend for XCapCaptureBackend {
    async fn capabilities(&self) -> CaptureCapabilities {
        CaptureCapabilities {
            targets: vec![CaptureTargetKind::Monitor, CaptureTargetKind::Window],
            permission: platform_permission(),
            message: None,
        }
    }

    async fn targets(&self, kind: CaptureTargetKind) -> Result<Vec<CaptureTarget>, CaptureError> {
        if is_wayland_session() {
            return Err(unsupported_error());
        }
        let targets = match kind {
            CaptureTargetKind::Monitor => Monitor::all()
                .map_err(map_xcap_error)?
                .into_iter()
                .map(|monitor| {
                    Ok(CaptureTarget {
                        id: format!("monitor:{}", monitor.id().map_err(map_xcap_error)?),
                        label: monitor
                            .friendly_name()
                            .or_else(|_| monitor.name())
                            .map_err(map_xcap_error)?,
                        kind,
                    })
                })
                .collect::<Result<Vec<_>, CaptureError>>()?,
            CaptureTargetKind::Window => Window::all()
                .map_err(map_xcap_error)?
                .into_iter()
                .filter_map(|window| {
                    let id = window.id().ok()?;
                    let title = window.title().ok()?;
                    if title.trim().is_empty() || window.is_minimized().unwrap_or(true) {
                        return None;
                    }
                    Some(CaptureTarget {
                        id: format!("window:{id}"),
                        label: title,
                        kind,
                    })
                })
                .collect(),
        };
        Ok(targets)
    }

    async fn capture(
        &self,
        target: &CaptureTarget,
        cancellation: CancellationToken,
    ) -> Result<RgbaImage, CaptureError> {
        if is_wayland_session() {
            return Err(unsupported_error());
        }
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        let image = match target.kind {
            CaptureTargetKind::Monitor => {
                let id = parse_target_id(&target.id, "monitor:")?;
                let monitor = Monitor::all()
                    .map_err(map_xcap_error)?
                    .into_iter()
                    .find(|monitor| monitor.id().ok() == Some(id))
                    .ok_or_else(|| {
                        CaptureError::new(
                            CaptureErrorKind::InvalidTarget,
                            "The selected screen is unavailable",
                        )
                    })?;
                monitor.capture_image().map_err(map_xcap_error)?
            }
            CaptureTargetKind::Window => {
                let id = parse_target_id(&target.id, "window:")?;
                let window = Window::all()
                    .map_err(map_xcap_error)?
                    .into_iter()
                    .find(|window| window.id().ok() == Some(id))
                    .ok_or_else(|| {
                        CaptureError::new(
                            CaptureErrorKind::InvalidTarget,
                            "The selected window is unavailable",
                        )
                    })?;
                window.capture_image().map_err(map_xcap_error)?
            }
        };
        if cancellation.is_cancelled() {
            return Err(CaptureError::new(
                CaptureErrorKind::Cancelled,
                "Screen capture was cancelled",
            ));
        }
        Ok(image)
    }

    async fn monitor_under_cursor(&self) -> Result<CaptureTarget, CaptureError> {
        let position = PlatformCursorPositionProvider.position()?;
        XCapMonitorResolver.resolve(position)
    }
}

#[must_use]
pub fn platform_capture_backend() -> Arc<dyn CaptureBackend> {
    #[cfg(target_os = "linux")]
    if is_wayland_session() {
        return Arc::new(super::portal::PortalCaptureBackend);
    }

    Arc::new(XCapCaptureBackend)
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

#[cfg(not(target_os = "linux"))]
const fn is_wayland_session() -> bool {
    false
}

const fn unsupported_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::Unsupported,
        "Screen capture is not supported on this desktop",
    )
}

const fn unavailable_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::Unavailable,
        "The screen under the pointer could not be resolved",
    )
}

#[cfg(target_os = "macos")]
const fn platform_permission() -> CapturePermission {
    CapturePermission::UserPrompt
}

#[cfg(not(target_os = "macos"))]
const fn platform_permission() -> CapturePermission {
    CapturePermission::NotRequired
}

fn parse_target_id(value: &str, prefix: &str) -> Result<u32, CaptureError> {
    value
        .strip_prefix(prefix)
        .and_then(|id| id.parse::<u32>().ok())
        .ok_or_else(|| {
            CaptureError::new(
                CaptureErrorKind::InvalidTarget,
                "The selected screen or window is unavailable",
            )
        })
}

fn map_xcap_error(error: XCapError) -> CaptureError {
    match error {
        XCapError::NotSupported => CaptureError::new(
            CaptureErrorKind::Unsupported,
            "Screen capture is not supported on this desktop",
        ),
        XCapError::InvalidCaptureRegion(_) => CaptureError::new(
            CaptureErrorKind::InvalidRegion,
            "The selected region is outside the captured image",
        ),
        other => {
            let category = other.to_string().to_ascii_lowercase();
            if category.contains("permission")
                || category.contains("not authorized")
                || category.contains("denied")
            {
                CaptureError::new(
                    CaptureErrorKind::PermissionDenied,
                    "Screen capture permission was denied",
                )
            } else {
                CaptureError::new(
                    CaptureErrorKind::Unavailable,
                    "The selected screen or window could not be captured",
                )
            }
        }
    }
}
