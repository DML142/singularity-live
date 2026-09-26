use std::{
    fmt,
    sync::{Arc, Mutex, MutexGuard, Weak},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CaptureError, CaptureErrorKind, CropRect, PreparedImage};

const DEFAULT_IMAGE_LIFETIME: Duration = Duration::from_mins(5);

#[derive(Clone, Copy, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CaptureId(Uuid);

impl CaptureId {
    /// Parses a capture identifier received from the desktop webview.
    ///
    /// # Errors
    ///
    /// Returns an expired-image error when the value is not a UUID.
    pub fn parse(value: &str) -> Result<Self, CaptureError> {
        Uuid::parse_str(value)
            .map(Self)
            .map_err(|_| expired_error())
    }
}

impl fmt::Debug for CaptureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("CaptureId([OPAQUE])")
    }
}

impl fmt::Display for CaptureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

pub struct CapturePreview {
    pub capture_id: CaptureId,
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub expires_in_seconds: u64,
}

impl fmt::Debug for CapturePreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturePreview")
            .field("capture_id", &self.capture_id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("expires_in_seconds", &self.expires_in_seconds)
            .field("data_url", &"[REDACTED]")
            .finish()
    }
}

pub struct TransientImageStore {
    lifetime: Duration,
    entry: Mutex<Option<StoredImage>>,
}

impl Default for TransientImageStore {
    fn default() -> Self {
        Self::new(DEFAULT_IMAGE_LIFETIME)
    }
}

impl TransientImageStore {
    #[must_use]
    pub fn new(lifetime: Duration) -> Self {
        Self {
            lifetime,
            entry: Mutex::new(None),
        }
    }

    /// Stores one image in memory and starts its bounded expiry timer.
    ///
    /// # Errors
    ///
    /// Returns an image preparation error when a preview cannot be generated.
    pub fn insert(self: &Arc<Self>, image: PreparedImage) -> Result<CapturePreview, CaptureError> {
        let entry = StoredImage {
            capture_id: CaptureId(Uuid::new_v4()),
            expires_at: Instant::now() + self.lifetime,
            image,
        };
        let preview = preview_of(&entry, Instant::now());
        let capture_id = entry.capture_id;
        *self.entry_lock() = Some(entry);
        self.schedule_expiry(capture_id);
        Ok(preview)
    }

    /// Returns a preview for the current image without exposing its canonical storage.
    ///
    /// # Errors
    ///
    /// Returns an expired-image error when the identifier is stale or expired.
    pub fn preview(&self, capture_id: &CaptureId) -> Result<CapturePreview, CaptureError> {
        let mut state = self.entry_lock();
        let Some(entry) = state.as_ref() else {
            return Err(expired_error());
        };
        if entry.expires_at <= Instant::now() {
            *state = None;
            return Err(expired_error());
        }
        if &entry.capture_id != capture_id {
            return Err(expired_error());
        }
        Ok(preview_of(entry, Instant::now()))
    }

    /// Removes and transfers the current image for one provider request.
    ///
    /// # Errors
    ///
    /// Returns an expired-image error when the identifier is stale or expired.
    pub fn take(&self, capture_id: &CaptureId) -> Result<PreparedImage, CaptureError> {
        let mut state = self.entry_lock();
        let Some(entry) = state.as_ref() else {
            return Err(expired_error());
        };
        if entry.expires_at <= Instant::now() {
            *state = None;
            return Err(expired_error());
        }
        if &entry.capture_id != capture_id {
            return Err(expired_error());
        }
        state
            .take()
            .map(|stored| stored.image)
            .ok_or_else(expired_error)
    }

    /// Crops and replaces the current image after validating the requested rectangle.
    ///
    /// # Errors
    ///
    /// Returns an expired-image, invalid-region, or preparation error.
    pub fn crop(
        &self,
        capture_id: &CaptureId,
        rect: CropRect,
    ) -> Result<CapturePreview, CaptureError> {
        let mut state = self.entry_lock();
        let Some(entry) = state.as_mut() else {
            return Err(expired_error());
        };
        if entry.expires_at <= Instant::now() {
            *state = None;
            return Err(expired_error());
        }
        if &entry.capture_id != capture_id {
            return Err(expired_error());
        }
        let cropped = entry.image.crop(rect)?;
        entry.image = cropped;
        Ok(preview_of(entry, Instant::now()))
    }

    /// Discards the current image when its identifier matches.
    ///
    /// # Errors
    ///
    /// Returns an expired-image error when the identifier is stale or expired.
    pub fn discard(&self, capture_id: &CaptureId) -> Result<(), CaptureError> {
        let mut state = self.entry_lock();
        let Some(entry) = state.as_ref() else {
            return Err(expired_error());
        };
        if entry.expires_at <= Instant::now() {
            *state = None;
            return Err(expired_error());
        }
        if &entry.capture_id != capture_id {
            return Err(expired_error());
        }
        *state = None;
        Ok(())
    }

    pub fn purge_expired(&self) -> bool {
        let mut state = self.entry_lock();
        if state
            .as_ref()
            .is_some_and(|entry| entry.expires_at <= Instant::now())
        {
            *state = None;
            true
        } else {
            false
        }
    }

    pub fn clear(&self) {
        *self.entry_lock() = None;
    }

    fn entry_lock(&self) -> MutexGuard<'_, Option<StoredImage>> {
        self.entry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn schedule_expiry(self: &Arc<Self>, capture_id: CaptureId) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let store: Weak<Self> = Arc::downgrade(self);
        let lifetime = self.lifetime;
        runtime.spawn(async move {
            tokio::time::sleep(lifetime).await;
            if let Some(store) = store.upgrade() {
                store.expire_if_current(capture_id);
            }
        });
    }

    fn expire_if_current(&self, capture_id: CaptureId) {
        let mut state = self.entry_lock();
        if state.as_ref().is_some_and(|entry| {
            entry.capture_id == capture_id && entry.expires_at <= Instant::now()
        }) {
            *state = None;
        }
    }
}

struct StoredImage {
    capture_id: CaptureId,
    expires_at: Instant,
    image: PreparedImage,
}

fn preview_of(entry: &StoredImage, now: Instant) -> CapturePreview {
    CapturePreview {
        capture_id: entry.capture_id,
        data_url: format!(
            "data:image/png;base64,{}",
            STANDARD.encode(entry.image.bytes())
        ),
        width: entry.image.width(),
        height: entry.image.height(),
        expires_in_seconds: entry.expires_at.saturating_duration_since(now).as_secs(),
    }
}

const fn expired_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::ImageExpired,
        "The screenshot expired or is no longer available",
    )
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use xcap::image::{Rgba, RgbaImage};

    use crate::capture::{CaptureErrorKind, CropRect, prepare_image};

    use super::{CaptureId, TransientImageStore};

    fn image() -> crate::capture::PreparedImage {
        prepare_image(RgbaImage::from_pixel(2, 2, Rgba([10, 20, 30, 255])))
            .expect("image can be prepared")
    }

    #[test]
    fn image_lives_in_memory_and_preview_is_redacted_from_debug() {
        let store = Arc::new(TransientImageStore::default());
        let preview = store.insert(image()).expect("image is stored");
        let data_url = preview.data_url.clone();

        assert!(data_url.starts_with("data:image/png;base64,"));
        assert!(format!("{preview:?}").contains("[REDACTED]"));
        assert!(store.preview(&preview.capture_id).is_ok());
    }

    #[test]
    fn replacing_an_image_invalidates_the_previous_capture_id() {
        let store = Arc::new(TransientImageStore::default());
        let previous = store.insert(image()).expect("first image is stored");
        let current = store.insert(image()).expect("second image replaces first");

        assert_eq!(
            store
                .preview(&previous.capture_id)
                .expect_err("previous capture is removed")
                .kind,
            CaptureErrorKind::ImageExpired
        );
        assert!(store.preview(&current.capture_id).is_ok());
    }

    #[test]
    fn expiry_discard_reset_and_take_remove_the_canonical_image() {
        let store = Arc::new(TransientImageStore::new(Duration::ZERO));
        let expired = store.insert(image()).expect("image is stored");
        assert!(store.purge_expired());
        assert!(store.preview(&expired.capture_id).is_err());

        let store = Arc::new(TransientImageStore::default());
        let discarded = store.insert(image()).expect("image is stored");
        store
            .discard(&discarded.capture_id)
            .expect("image is discarded");
        assert!(store.preview(&discarded.capture_id).is_err());

        let sent = store.insert(image()).expect("image is stored");
        let consumed = store
            .take(&sent.capture_id)
            .expect("image is consumed once");
        assert_eq!((consumed.width(), consumed.height()), (2, 2));
        assert!(store.take(&sent.capture_id).is_err());

        let reset = store.insert(image()).expect("image is stored");
        store.clear();
        assert!(store.preview(&reset.capture_id).is_err());
    }

    #[test]
    fn stale_or_cross_session_capture_ids_cannot_access_the_current_image() {
        let store = Arc::new(TransientImageStore::default());
        let stale =
            CaptureId::parse("00000000-0000-0000-0000-000000000001").expect("uuid is well formed");
        let current = store.insert(image()).expect("image is stored");

        assert_eq!(
            store
                .preview(&stale)
                .expect_err("unknown capture ID is rejected")
                .kind,
            CaptureErrorKind::ImageExpired
        );
        assert!(store.preview(&current.capture_id).is_ok());
    }

    #[test]
    fn crop_replaces_the_stored_preview_after_validation() {
        let store = Arc::new(TransientImageStore::default());
        let preview = store.insert(image()).expect("image is stored");

        let cropped = store
            .crop(
                &preview.capture_id,
                CropRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 2,
                },
            )
            .expect("valid region is cropped");

        assert_eq!((cropped.width, cropped.height), (1, 2));
    }

    #[tokio::test]
    async fn ttl_worker_removes_image_bytes_without_a_follow_up_command() {
        let store = Arc::new(TransientImageStore::new(Duration::from_millis(10)));
        let preview = store.insert(image()).expect("image is stored");

        tokio::time::sleep(Duration::from_millis(30)).await;

        assert!(store.preview(&preview.capture_id).is_err());
    }
}
