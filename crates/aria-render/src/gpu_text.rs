use glyphon::{
    Cache, FontSystem, Resolution, SwashCache, TextArea, TextAtlas, TextRenderer, Viewport,
    cosmic_text::Fallback, fontdb,
};
use thiserror::Error;

/// Raw bytes for one explicitly bundled font asset.
///
/// The logical path is retained solely for useful diagnostics; all shaping is
/// performed from these bytes, never from the host's installed fonts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledFont {
    pub logical_path: String,
    pub bytes: Vec<u8>,
}

/// Failure while preparing the bundled-only text system.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BundledFontError {
    #[error("bundled font '{logical_path}' does not contain a readable OpenType/TrueType face")]
    Invalid { logical_path: String },
}

/// Disables cosmic-text's platform fallback lists. The database contains only
/// `BundledFont` bytes, so glyph fallback is deterministic and cannot select
/// Segoe/Yu Gothic on Windows or Noto/DejaVu on Linux.
#[derive(Debug)]
struct BundledOnlyFallback;

impl Fallback for BundledOnlyFallback {
    fn common_fallback(&self) -> &'static [&'static str] {
        &[]
    }

    fn forbidden_fallback(&self) -> &'static [&'static str] {
        &[]
    }

    fn script_fallback(&self, _: unicode_script::Script, _: &str) -> &'static [&'static str] {
        &[]
    }
}

/// Owns glyphon's GPU resources while callers own individual text buffers.
pub struct GpuTextLayer {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
}

impl std::fmt::Debug for GpuTextLayer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GpuTextLayer")
            .finish_non_exhaustive()
    }
}

impl GpuTextLayer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        fonts: Vec<BundledFont>,
    ) -> Result<Self, BundledFontError> {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let renderer =
            TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
        Ok(Self {
            font_system: bundled_font_system(fonts)?,
            swash_cache: SwashCache::new(),
            viewport,
            atlas,
            renderer,
        })
    }

    #[must_use]
    pub fn has_fonts(&self) -> bool {
        self.font_system.db().faces().next().is_some()
    }

    pub fn prepare<'a>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        areas: impl IntoIterator<Item = TextArea<'a>>,
    ) -> Result<(), glyphon::PrepareError> {
        self.viewport.update(queue, Resolution { width, height });
        self.renderer.prepare(
            device,
            queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        )
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) -> Result<(), glyphon::RenderError> {
        self.renderer.render(&self.atlas, &self.viewport, pass)
    }

    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}

fn bundled_font_system(fonts: Vec<BundledFont>) -> Result<FontSystem, BundledFontError> {
    let mut database = fontdb::Database::new();
    let mut primary_family = None;
    for font in fonts {
        let before = database.len();
        database.load_font_data(font.bytes);
        if database.len() == before {
            return Err(BundledFontError::Invalid {
                logical_path: font.logical_path,
            });
        }
        if primary_family.is_none() {
            primary_family = database
                .faces()
                .skip(before)
                .find_map(|face| face.families.first().map(|(name, _)| name.clone()));
        }
    }
    if let Some(primary_family) = primary_family {
        database.set_sans_serif_family(primary_family);
    }
    // A fixed locale avoids a user environment changing font selection. The
    // custom fallback above makes that choice independent of OS fallback
    // lists; the locale is retained only as deterministic shaping metadata.
    Ok(FontSystem::new_with_locale_and_db_and_fallback(
        "en-US".to_owned(),
        database,
        BundledOnlyFallback,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bundles_do_not_discover_system_fonts() {
        let system = bundled_font_system(Vec::new()).unwrap();
        assert_eq!(system.db().len(), 0);
    }

    #[test]
    fn invalid_bundled_font_is_rejected() {
        let error = bundled_font_system(vec![BundledFont {
            logical_path: "assets/fonts/broken.ttf".to_owned(),
            bytes: b"not a font".to_vec(),
        }])
        .unwrap_err();
        assert!(matches!(error, BundledFontError::Invalid { .. }));
    }
}
