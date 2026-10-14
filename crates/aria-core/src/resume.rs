//! Relocatable resume points.
//!
//! A snapshot's program counter is only meaningful for the exact bytecode that
//! produced it. Novel scripts are patched after release (a typo, a new line),
//! so a save also records where it is in terms the compiler keeps stable:
//! scene-start, choice, and call-return anchors (`Nop` with a name) and line
//! keys on `Text`. Loading into a rebuilt program maps those back to
//! addresses; loading into the same program keeps the exact counter.

use serde::{Deserialize, Serialize};

use crate::bytecode::{ByteOp, CompiledProgram, Constant, Operand, SCENE_ANCHOR_PREFIX};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    Scene,
    Line,
    Choice,
    Return,
}

/// One stable position, independent of instruction addresses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorRef {
    pub scene: String,
    pub kind: AnchorKind,
    /// Line key or anchor name, unique within the program.
    pub name: String,
    /// Position among anchors of the same kind in the scene; the fallback
    /// when the exact line was edited.
    pub ordinal: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePoint {
    /// Digest of the bytecode that wrote the save.
    pub program: String,
    pub at: AnchorRef,
    pub call_stack: Vec<AnchorRef>,
}

#[derive(Debug, Clone)]
struct Anchor {
    address: u32,
    reference: AnchorRef,
}

/// Every anchor in program order, built once per loaded program.
#[derive(Debug, Clone, Default)]
pub struct AnchorIndex {
    anchors: Vec<Anchor>,
}

impl AnchorIndex {
    #[must_use]
    pub fn build(program: &CompiledProgram) -> Self {
        let mut anchors = Vec::new();
        let mut scene = String::new();
        let mut ordinals: std::collections::BTreeMap<AnchorKind, u32> = Default::default();
        for (address, instruction) in program.instructions.iter().enumerate() {
            let Ok(address) = u32::try_from(address) else {
                break;
            };
            let (kind, name) = match instruction.op {
                ByteOp::Nop => match instruction
                    .operands
                    .first()
                    .and_then(|operand| string(program, operand))
                {
                    Some(name) => match name.split_once(':').map(|(kind, _)| kind) {
                        Some("scene") => (AnchorKind::Scene, name),
                        Some("choice") => (AnchorKind::Choice, name),
                        Some("return") => (AnchorKind::Return, name),
                        _ => continue,
                    },
                    None => continue,
                },
                ByteOp::Text => match instruction
                    .operands
                    .get(2)
                    .and_then(|operand| string(program, operand))
                {
                    Some(key) => (AnchorKind::Line, key),
                    None => continue,
                },
                _ => continue,
            };
            if kind == AnchorKind::Scene {
                scene = name[SCENE_ANCHOR_PREFIX.len()..].to_owned();
                ordinals.clear();
            }
            let ordinal = ordinals.entry(kind).or_insert(0);
            anchors.push(Anchor {
                address,
                reference: AnchorRef {
                    scene: scene.clone(),
                    kind,
                    name,
                    ordinal: *ordinal,
                },
            });
            *ordinal += 1;
        }
        Self { anchors }
    }

    /// The nearest anchor at or before `address`.
    #[must_use]
    pub fn at_or_before(&self, address: u32) -> Option<&AnchorRef> {
        let index = self
            .anchors
            .partition_point(|anchor| anchor.address <= address);
        index
            .checked_sub(1)
            .map(|index| &self.anchors[index].reference)
    }

    /// Maps a saved anchor into this program: the exact anchor, then the same
    /// ordinal of that kind in the same scene, then the scene's start.
    #[must_use]
    pub fn locate(&self, reference: &AnchorRef) -> Option<u32> {
        let exact = self
            .anchors
            .iter()
            .find(|anchor| anchor.reference.name == reference.name)
            .map(|anchor| anchor.address);
        if exact.is_some() {
            return exact;
        }
        let in_scene = |kind: AnchorKind, ordinal: Option<u32>| {
            self.anchors
                .iter()
                .find(|anchor| {
                    anchor.reference.scene == reference.scene
                        && anchor.reference.kind == kind
                        && ordinal.is_none_or(|ordinal| anchor.reference.ordinal == ordinal)
                })
                .map(|anchor| anchor.address)
        };
        in_scene(reference.kind, Some(reference.ordinal))
            .or_else(|| in_scene(AnchorKind::Scene, None))
    }

    /// A call-return anchor must resolve exactly by kind and ordinal; falling
    /// back to a scene start would resume the caller from the wrong place.
    #[must_use]
    pub fn locate_return(&self, reference: &AnchorRef) -> Option<u32> {
        self.anchors
            .iter()
            .find(|anchor| {
                anchor.reference.kind == AnchorKind::Return
                    && (anchor.reference.name == reference.name
                        || (anchor.reference.scene == reference.scene
                            && anchor.reference.ordinal == reference.ordinal))
            })
            .map(|anchor| anchor.address)
    }
}

fn string(program: &CompiledProgram, operand: &Operand) -> Option<String> {
    let Operand::Constant(index) = operand else {
        return None;
    };
    match program.constants.get(usize::try_from(*index).ok()?)? {
        Constant::String(value) => Some(value.clone()),
        _ => None,
    }
}

/// A digest that changes whenever any instruction or constant does.
#[must_use]
pub fn program_digest(program: &CompiledProgram) -> String {
    program
        .encode()
        .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
        .unwrap_or_default()
}
