use crate::capture::{CaptureError, CaptureErrorKind, CaptureTarget, CaptureTargetKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub trait CursorPositionProvider: Send + Sync {
    /// Returns the current pointer location in virtual desktop coordinates.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error when the desktop does not expose pointer coordinates.
    fn position(&self) -> Result<CursorPosition, CaptureError>;
}

pub trait MonitorResolver: Send + Sync {
    /// Resolves the monitor containing the supplied virtual desktop coordinate.
    ///
    /// # Errors
    ///
    /// Returns an unavailable error if no monitor contains the coordinate or monitor metadata
    /// cannot be read.
    fn resolve(&self, position: CursorPosition) -> Result<CaptureTarget, CaptureError>;
}

pub(crate) struct PlatformCursorPositionProvider;

impl CursorPositionProvider for PlatformCursorPositionProvider {
    fn position(&self) -> Result<CursorPosition, CaptureError> {
        #[cfg(target_os = "linux")]
        if is_wayland_session() {
            return Err(unavailable_error());
        }

        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        {
            let enigo =
                enigo::Enigo::new(&enigo::Settings::default()).map_err(|_| unavailable_error())?;
            let (x, y) = enigo::Mouse::location(&enigo).map_err(|_| unavailable_error())?;
            return Ok(CursorPosition { x, y });
        }

        #[allow(unreachable_code)]
        Err(CaptureError::new(
            CaptureErrorKind::Unsupported,
            "Pointer position is unavailable on this desktop",
        ))
    }
}

pub(crate) struct XCapMonitorResolver;

impl MonitorResolver for XCapMonitorResolver {
    fn resolve(&self, position: CursorPosition) -> Result<CaptureTarget, CaptureError> {
        let monitors = xcap::Monitor::all().map_err(|_| unavailable_error())?;
        let mut candidates = Vec::with_capacity(monitors.len());
        let mut bounds = Vec::with_capacity(monitors.len());
        for monitor in monitors {
            bounds.push(MonitorBounds {
                x: monitor.x().map_err(|_| unavailable_error())?,
                y: monitor.y().map_err(|_| unavailable_error())?,
                width: monitor.width().map_err(|_| unavailable_error())?,
                height: monitor.height().map_err(|_| unavailable_error())?,
            });
            let id = monitor.id().map_err(|_| unavailable_error())?;
            let label = monitor
                .friendly_name()
                .or_else(|_| monitor.name())
                .map_err(|_| unavailable_error())?;
            candidates.push(CaptureTarget {
                id: format!("monitor:{id}"),
                label,
                kind: CaptureTargetKind::Monitor,
            });
        }
        let index = monitor_containing(position, &bounds).ok_or_else(unavailable_error)?;
        candidates
            .into_iter()
            .nth(index)
            .ok_or_else(unavailable_error)
    }
}

fn monitor_containing(position: CursorPosition, monitors: &[MonitorBounds]) -> Option<usize> {
    monitors.iter().position(|monitor| {
        let left = i64::from(monitor.x);
        let top = i64::from(monitor.y);
        let right = left + i64::from(monitor.width);
        let bottom = top + i64::from(monitor.height);
        let x = i64::from(position.x);
        let y = i64::from(position.y);
        x >= left && x < right && y >= top && y < bottom
    })
}

#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    std::env::var_os("XDG_SESSION_TYPE").is_some_and(|value| value == "wayland")
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

const fn unavailable_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::Unavailable,
        "The screen under the pointer could not be resolved",
    )
}

#[cfg(test)]
mod tests {
    use super::{CursorPosition, MonitorBounds, monitor_containing};

    #[test]
    fn monitor_lookup_uses_half_open_bounds_and_supports_negative_origins() {
        let monitors = [
            MonitorBounds {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            MonitorBounds {
                x: -1280,
                y: 0,
                width: 1280,
                height: 1024,
            },
        ];

        assert_eq!(
            monitor_containing(CursorPosition { x: -1, y: 20 }, &monitors),
            Some(1)
        );
        assert_eq!(
            monitor_containing(CursorPosition { x: 0, y: 0 }, &monitors),
            Some(0)
        );
        assert_eq!(
            monitor_containing(CursorPosition { x: 1920, y: 0 }, &monitors),
            None
        );
    }
}
