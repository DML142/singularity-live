use std::{fmt, io::Cursor};

use xcap::image::{DynamicImage, ImageFormat, RgbaImage, imageops::FilterType};

use super::CaptureError;
use super::backend::CaptureErrorKind;

const MAX_SOURCE_PIXELS: u64 = 50_000_000;
const MAX_DIMENSION: u32 = 3_840;
const MAX_ENCODED_BYTES: usize = 8 * 1024 * 1024;
const MAX_ENCODE_ATTEMPTS: usize = 8;

pub struct PreparedImage {
    png: Vec<u8>,
    width: u32,
    height: u32,
}

impl PreparedImage {
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.png
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.png
    }

    /// Returns a normalized PNG containing only the selected area.
    ///
    /// # Errors
    ///
    /// Returns an invalid-region error when the rectangle is empty or outside this image.
    pub fn crop(&self, rect: CropRect) -> Result<Self, CaptureError> {
        validate_crop(rect, self.width, self.height)?;
        let decoded = xcap::image::load_from_memory_with_format(&self.png, ImageFormat::Png)
            .map_err(|_| preparation_error())?
            .to_rgba8();
        let cropped =
            xcap::image::imageops::crop_imm(&decoded, rect.x, rect.y, rect.width, rect.height)
                .to_image();
        prepare_image(cropped)
    }
}

impl fmt::Debug for PreparedImage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("byte_count", &self.png.len())
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Bounds and encodes captured pixels as an in-memory PNG.
///
/// # Errors
///
/// Returns a preparation error when the source is too large or cannot be encoded within the
/// configured limits.
pub fn prepare_image(pixels: RgbaImage) -> Result<PreparedImage, CaptureError> {
    let width = pixels.width();
    let height = pixels.height();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_SOURCE_PIXELS {
        return Err(preparation_error());
    }

    let mut prepared_pixels = if width > MAX_DIMENSION || height > MAX_DIMENSION {
        DynamicImage::ImageRgba8(pixels)
            .thumbnail(MAX_DIMENSION, MAX_DIMENSION)
            .to_rgba8()
    } else {
        pixels
    };
    for _ in 0..MAX_ENCODE_ATTEMPTS {
        let png = encode_png(&prepared_pixels)?;
        if png.len() <= MAX_ENCODED_BYTES {
            return Ok(PreparedImage {
                width: prepared_pixels.width(),
                height: prepared_pixels.height(),
                png,
            });
        }
        let width = (prepared_pixels.width() * 4 / 5).max(1);
        let height = (prepared_pixels.height() * 4 / 5).max(1);
        prepared_pixels =
            xcap::image::imageops::resize(&prepared_pixels, width, height, FilterType::Triangle);
    }
    Err(preparation_error())
}

fn encode_png(pixels: &RgbaImage) -> Result<Vec<u8>, CaptureError> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels.clone())
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|_| preparation_error())?;
    Ok(cursor.into_inner())
}

fn validate_crop(rect: CropRect, image_width: u32, image_height: u32) -> Result<(), CaptureError> {
    let right = rect.x.checked_add(rect.width);
    let bottom = rect.y.checked_add(rect.height);
    if rect.width == 0
        || rect.height == 0
        || right.is_none_or(|right| right > image_width)
        || bottom.is_none_or(|bottom| bottom > image_height)
    {
        return Err(CaptureError::new(
            CaptureErrorKind::InvalidRegion,
            "The selected region is outside the screenshot",
        ));
    }
    Ok(())
}

const fn preparation_error() -> CaptureError {
    CaptureError::new(
        CaptureErrorKind::Preparation,
        "The screenshot could not be prepared",
    )
}

#[cfg(test)]
mod tests {
    use xcap::image::{Rgba, RgbaImage};

    use super::{CaptureErrorKind, CropRect, prepare_image};

    #[test]
    fn crops_a_valid_in_bounds_region() {
        let image = prepare_image(RgbaImage::from_pixel(4, 3, Rgba([8, 9, 10, 255])))
            .expect("image can be prepared");

        let cropped = image
            .crop(CropRect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            })
            .expect("valid crop succeeds");

        assert_eq!((cropped.width(), cropped.height()), (2, 2));
    }

    #[test]
    fn rejects_empty_negative_overflowed_and_out_of_bounds_regions() {
        let image = prepare_image(RgbaImage::from_pixel(4, 3, Rgba([8, 9, 10, 255])))
            .expect("image can be prepared");
        for rect in [
            CropRect {
                x: 0,
                y: 0,
                width: 0,
                height: 1,
            },
            CropRect {
                x: 0,
                y: 0,
                width: 1,
                height: 0,
            },
            CropRect {
                x: u32::MAX,
                y: 0,
                width: 2,
                height: 1,
            },
            CropRect {
                x: 3,
                y: 0,
                width: 2,
                height: 1,
            },
            CropRect {
                x: 0,
                y: 2,
                width: 1,
                height: 2,
            },
        ] {
            let error = image.crop(rect).expect_err("invalid crop is rejected");
            assert_eq!(error.kind, CaptureErrorKind::InvalidRegion);
        }
    }

    #[test]
    fn prepared_image_debug_output_redacts_png_bytes() {
        let image = prepare_image(RgbaImage::from_pixel(2, 2, Rgba([8, 9, 10, 255])))
            .expect("image can be prepared");
        let debug = format!("{image:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("89500a"));
    }
}
