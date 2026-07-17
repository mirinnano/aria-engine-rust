use aria_core::protocol::{BlendMode, DrawCommand, RenderFrame};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveKind {
    Sprite,
    Rectangle,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchKey {
    pub primitive: PrimitiveKind,
    pub asset: Option<String>,
    pub blend: Option<BlendMode>,
    pub mask: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderBatch {
    pub key: BatchKey,
    pub commands: Vec<DrawCommand>,
}

/// Groups consecutive compatible commands. It never reorders transparent
/// commands, preserving the core's stable z/id ordering.
#[must_use]
pub fn build_batches(frame: &RenderFrame) -> Vec<RenderBatch> {
    let mut batches: Vec<RenderBatch> = Vec::new();
    for command in &frame.commands {
        let key = key_for(command);
        if let Some(batch) = batches.last_mut()
            && batch.key == key
        {
            batch.commands.push(command.clone());
            continue;
        }
        batches.push(RenderBatch {
            key,
            commands: vec![command.clone()],
        });
    }
    batches
}

fn key_for(command: &DrawCommand) -> BatchKey {
    match command {
        DrawCommand::Sprite {
            asset, blend, mask, ..
        } => BatchKey {
            primitive: PrimitiveKind::Sprite,
            asset: Some(asset.clone()),
            blend: Some(*blend),
            mask: mask.clone(),
        },
        DrawCommand::Rectangle { .. } => BatchKey {
            primitive: PrimitiveKind::Rectangle,
            asset: None,
            blend: None,
            mask: None,
        },
        DrawCommand::Text { .. } => BatchKey {
            primitive: PrimitiveKind::Text,
            asset: None,
            blend: None,
            mask: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use aria_core::protocol::{Color, LogicalSize, Rect};

    use super::*;

    #[test]
    fn batching_never_crosses_an_intervening_z_ordered_primitive() {
        let sprite = |id: &str| DrawCommand::Sprite {
            id: id.to_owned(),
            asset: "atlas.webp".to_owned(),
            destination: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            opacity: 255,
            z: 0,
            visible: true,
            blend: BlendMode::Alpha,
            mask: None,
        };
        let frame = RenderFrame {
            frame_number: 1,
            logical_size: LogicalSize {
                width: 1280,
                height: 720,
            },
            clear_color: Color::BLACK,
            commands: vec![
                sprite("a"),
                sprite("b"),
                DrawCommand::Rectangle {
                    id: "overlay".to_owned(),
                    bounds: Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    color: Color::WHITE,
                    corner_radius: 0.0,
                    z: 1,
                },
                sprite("c"),
            ],
            transition: None,
        };
        let batches = build_batches(&frame);
        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].commands.len(), 2);
        assert_eq!(batches[2].commands.len(), 1);
    }
}
