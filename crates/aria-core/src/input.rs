use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputAction {
    NavigateUp,
    NavigateDown,
    NavigateLeft,
    NavigateRight,
    Confirm,
    Cancel,
    Menu,
    Advance,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PointerSnapshot {
    pub x: f32,
    pub y: f32,
    pub primary_pressed: bool,
    pub primary_held: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSnapshot {
    pub sequence: u64,
    pub delta_ms: u32,
    #[serde(default)]
    pub pressed: BTreeSet<InputAction>,
    #[serde(default)]
    pub held: BTreeSet<InputAction>,
    pub pointer: Option<PointerSnapshot>,
}

impl InputSnapshot {
    #[must_use]
    pub fn idle(sequence: u64, delta_ms: u32) -> Self {
        Self {
            sequence,
            delta_ms,
            pressed: BTreeSet::new(),
            held: BTreeSet::new(),
            pointer: None,
        }
    }

    #[must_use]
    pub fn pressed(sequence: u64, delta_ms: u32, action: InputAction) -> Self {
        Self {
            sequence,
            delta_ms,
            pressed: BTreeSet::from([action]),
            held: BTreeSet::new(),
            pointer: None,
        }
    }

    #[must_use]
    pub fn is_pressed(&self, action: InputAction) -> bool {
        self.pressed.contains(&action)
    }

    #[must_use]
    pub fn is_held(&self, action: InputAction) -> bool {
        self.held.contains(&action)
    }
}
