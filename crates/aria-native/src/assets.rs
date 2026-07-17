//! Native asset decoding behind the logical-path boundary used by the Core.

use std::collections::BTreeMap;
use std::io::Cursor;

use aria_render::{ImageResolver, RasterImage};

const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 64 * 1024 * 1024;

/// Reads logical asset paths from either an unpacked project or a pak-backed
/// package. Implementations must reject paths outside their declared root.
pub trait AssetProvider {
    fn read_asset(&mut self, logical_path: &str) -> Result<Vec<u8>, String>;
}

/// Cached asset transport plus image/SVG decoding for the Native renderer.
pub struct NativeAssetStore {
    provider: Box<dyn AssetProvider>,
    bytes: BTreeMap<String, Vec<u8>>,
}

impl std::fmt::Debug for NativeAssetStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeAssetStore")
            .field("cached_asset_count", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl NativeAssetStore {
    #[must_use]
    pub fn new(provider: Box<dyn AssetProvider>) -> Self {
        Self {
            provider,
            bytes: BTreeMap::new(),
        }
    }

    pub fn read(&mut self, logical_path: &str) -> Result<Vec<u8>, String> {
        if let Some(bytes) = self.bytes.get(logical_path) {
            return Ok(bytes.clone());
        }
        let bytes = self.provider.read_asset(logical_path)?;
        self.bytes.insert(logical_path.to_owned(), bytes.clone());
        Ok(bytes)
    }
}

impl ImageResolver for NativeAssetStore {
    fn load_image(
        &mut self,
        logical_path: &str,
        desired_size: Option<(u32, u32)>,
    ) -> Result<RasterImage, String> {
        let bytes = self.read(logical_path)?;
        if is_svg(logical_path, &bytes) {
            decode_svg(&bytes, desired_size)
        } else {
            decode_raster(&bytes)
        }
    }
}

fn is_svg(logical_path: &str, bytes: &[u8]) -> bool {
    logical_path
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("svg"))
        || bytes
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .take(512)
            .collect::<Vec<_>>()
            .windows(4)
            .any(|window| window.eq_ignore_ascii_case(b"<svg"))
}

fn decode_raster(bytes: &[u8]) -> Result<RasterImage, String> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| format!("cannot identify raster image: {error}"))?;
    let image = reader
        .decode()
        .map_err(|error| format!("cannot decode raster image: {error}"))?
        .to_rgba8();
    validate_size(image.width(), image.height())?;
    RasterImage::new(image.width(), image.height(), image.into_raw())
        .map_err(|error| error.to_string())
}

fn decode_svg(bytes: &[u8], desired_size: Option<(u32, u32)>) -> Result<RasterImage, String> {
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &options)
        .map_err(|error| format!("cannot parse SVG: {error}"))?;
    let intrinsic = tree.size().to_int_size();
    let (width, height) = desired_size.unwrap_or((intrinsic.width(), intrinsic.height()));
    validate_size(width, height)?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("cannot allocate SVG raster {width}x{height}"))?;
    let transform = resvg::tiny_skia::Transform::from_scale(
        width as f32 / intrinsic.width() as f32,
        height as f32 / intrinsic.height() as f32,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let mut rgba = pixmap.data().to_vec();
    unpremultiply_rgba(&mut rgba);
    RasterImage::new(width, height, rgba).map_err(|error| error.to_string())
}

fn validate_size(width: u32, height: u32) -> Result<(), String> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || pixels > MAX_PIXELS
    {
        Err(format!(
            "image dimensions {width}x{height} exceed the Native player limit"
        ))
    } else {
        Ok(())
    }
}

fn unpremultiply_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
        } else if alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_svg_pixels_become_straight_alpha() {
        let mut pixels = vec![64, 32, 16, 128, 9, 8, 7, 0];
        unpremultiply_rgba(&mut pixels);
        assert_eq!(&pixels[..4], &[128, 64, 32, 128]);
        assert_eq!(&pixels[4..], &[0, 0, 0, 0]);
    }

    #[test]
    fn svg_detection_accepts_extension_and_document_prefix() {
        assert!(is_svg("art/scene.svg", b"not actually XML"));
        assert!(is_svg("art/scene.dat", b" \n<svg viewBox=\"0 0 1 1\"/>"));
        assert!(!is_svg("art/scene.png", b"\x89PNG\r\n"));
    }
}
