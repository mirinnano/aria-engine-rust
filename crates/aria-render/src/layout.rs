use aria_core::protocol::{LogicalSize, Rect};
use serde::{Deserialize, Serialize};
use taffy::prelude::*;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct SafeAreaInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ViewportTransform {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
    pub logical_safe_area: Rect,
    pub minimum_target_size: f32,
}

impl ViewportTransform {
    #[must_use]
    pub fn fit(
        logical: LogicalSize,
        physical_width: u32,
        physical_height: u32,
        dpi_scale: f32,
        safe_area: SafeAreaInsets,
    ) -> Self {
        let available_width = (physical_width as f32 - safe_area.left - safe_area.right).max(1.0);
        let available_height = (physical_height as f32 - safe_area.top - safe_area.bottom).max(1.0);
        let scale = (available_width / logical.width.max(1) as f32)
            .min(available_height / logical.height.max(1) as f32)
            .max(f32::EPSILON);
        let rendered_width = logical.width as f32 * scale;
        let rendered_height = logical.height as f32 * scale;
        let offset_x = safe_area.left + (available_width - rendered_width) / 2.0;
        let offset_y = safe_area.top + (available_height - rendered_height) / 2.0;
        let logical_safe_area = Rect {
            x: ((safe_area.left - offset_x) / scale).max(0.0),
            y: ((safe_area.top - offset_y) / scale).max(0.0),
            width: available_width / scale,
            height: available_height / scale,
        };
        // 44 device-independent pixels is the shared minimum action target.
        let minimum_target_size = (44.0 * dpi_scale.max(1.0) / scale).max(44.0);
        Self {
            scale,
            offset_x,
            offset_y,
            logical_safe_area,
            minimum_target_size,
        }
    }

    #[must_use]
    pub fn physical_to_logical(self, x: f32, y: f32) -> (f32, f32) {
        (
            (x - self.offset_x) / self.scale,
            (y - self.offset_y) / self.scale,
        )
    }
}

#[derive(Debug, Default)]
pub struct UiLayoutEngine;

impl UiLayoutEngine {
    /// Computes a centered Flexbox menu using Taffy. Returned rectangles are in
    /// the same logical coordinate system used by the core UI tree.
    pub fn vertical_menu(
        &self,
        viewport: Rect,
        item_count: usize,
        preferred_width: f32,
        preferred_height: f32,
        minimum_target: f32,
        gap: f32,
    ) -> Result<Vec<Rect>, LayoutError> {
        if item_count == 0 {
            return Ok(Vec::new());
        }
        let mut taffy: TaffyTree<()> = TaffyTree::new();
        let item_height = preferred_height.max(minimum_target);
        let item_width = preferred_width.min(viewport.width).max(minimum_target);
        let mut children = Vec::with_capacity(item_count);
        for _ in 0..item_count {
            children.push(taffy.new_leaf(Style {
                size: Size {
                    width: Dimension::from_length(item_width),
                    height: Dimension::from_length(item_height),
                },
                min_size: Size {
                    width: Dimension::from_length(minimum_target),
                    height: Dimension::from_length(minimum_target),
                },
                ..Default::default()
            })?);
        }
        let root = taffy.new_with_children(
            Style {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                align_items: Some(AlignItems::CENTER),
                justify_content: Some(JustifyContent::CENTER),
                gap: Size {
                    width: LengthPercentage::length(0.0),
                    height: LengthPercentage::length(gap.max(0.0)),
                },
                size: Size {
                    width: Dimension::from_length(viewport.width),
                    height: Dimension::from_length(viewport.height),
                },
                ..Default::default()
            },
            &children,
        )?;
        taffy.compute_layout(
            root,
            Size {
                width: AvailableSpace::Definite(viewport.width),
                height: AvailableSpace::Definite(viewport.height),
            },
        )?;
        children
            .into_iter()
            .map(|node| {
                let layout = taffy.layout(node)?;
                Ok(Rect {
                    x: viewport.x + layout.location.x,
                    y: viewport.y + layout.location.y,
                    width: layout.size.width,
                    height: layout.size.height,
                })
            })
            .collect()
    }
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("Taffy layout failed: {0}")]
    Taffy(#[from] taffy::TaffyError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_respects_minimum_target_and_safe_viewport() {
        let viewport = Rect {
            x: 20.0,
            y: 10.0,
            width: 600.0,
            height: 400.0,
        };
        let items = UiLayoutEngine
            .vertical_menu(viewport, 3, 360.0, 24.0, 48.0, 12.0)
            .unwrap();
        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|item| item.height >= 48.0));
        assert!(items.windows(2).all(|pair| pair[0].y < pair[1].y));
        assert!(items.iter().all(|item| item.x >= viewport.x));
    }

    #[test]
    fn viewport_transform_letterboxes_ultrawide_and_round_trips_pointer() {
        let transform = ViewportTransform::fit(
            LogicalSize {
                width: 1280,
                height: 720,
            },
            2560,
            1080,
            1.5,
            SafeAreaInsets::default(),
        );
        assert!(transform.offset_x > 0.0);
        let physical_x = transform.offset_x + 640.0 * transform.scale;
        let physical_y = transform.offset_y + 360.0 * transform.scale;
        let logical = transform.physical_to_logical(physical_x, physical_y);
        assert!((logical.0 - 640.0).abs() < 0.001);
        assert!((logical.1 - 360.0).abs() < 0.001);
    }
}
