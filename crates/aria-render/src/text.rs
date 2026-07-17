use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, fontdb};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct TextShaper {
    font_system: FontSystem,
}

impl Default for TextShaper {
    fn default() -> Self {
        Self {
            // Never scan host fonts: this helper is used for deterministic
            // layout checks as well as rendering, so Windows/Linux must start
            // from the same empty bundled-font database.
            font_system: FontSystem::new_with_locale_and_db(
                "en-US".to_owned(),
                fontdb::Database::new(),
            ),
        }
    }
}

impl TextShaper {
    pub fn load_font(&mut self, bytes: Vec<u8>) {
        self.font_system.db_mut().load_font_data(bytes);
    }

    #[must_use]
    pub fn shape(
        &mut self,
        text: &str,
        font_size: f32,
        line_height: f32,
        width: f32,
        height: f32,
    ) -> ShapedText {
        let mut buffer = Buffer::new(
            &mut self.font_system,
            Metrics::new(font_size.max(1.0), line_height.max(font_size)),
        );
        buffer.set_size(Some(width.max(1.0)), Some(height.max(1.0)));
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut self.font_system, false);

        let mut lines = Vec::new();
        let mut glyphs = Vec::new();
        let mut missing_glyphs = 0;
        for (run_index, run) in buffer.layout_runs().enumerate() {
            lines.push(ShapedLine {
                index: run_index,
                width: run.line_w,
                top: run.line_top,
                height: run.line_height,
                rtl: run.rtl,
            });
            for glyph in run.glyphs {
                missing_glyphs += usize::from(glyph.glyph_id == 0);
                glyphs.push(ShapedGlyph {
                    line: run_index,
                    glyph_id: glyph.glyph_id,
                    byte_start: glyph.start,
                    byte_end: glyph.end,
                    x: glyph.x,
                    y: glyph.y,
                    width: glyph.w,
                });
            }
        }
        ShapedText {
            lines,
            glyphs,
            missing_glyphs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedText {
    pub lines: Vec<ShapedLine>,
    pub glyphs: Vec<ShapedGlyph>,
    pub missing_glyphs: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedLine {
    pub index: usize,
    pub width: f32,
    pub top: f32,
    pub height: f32,
    pub rtl: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShapedGlyph {
    pub line: usize,
    pub glyph_id: u16,
    pub byte_start: usize,
    pub byte_end: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
}
