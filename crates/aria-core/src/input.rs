use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::presentation::UiIntent;
use crate::presentation_state::UiViewport;

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
    ToggleAuto,
    ToggleSkip,
    OpenBacklog,
    QuickSave,
    QuickLoad,
    ReturnToTitle,
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
    /// Logical vertical scroll delta for list slots. It is recorded with the
    /// input rather than read from a host event queue so backlog/list replay
    /// remains deterministic across native and web adapters.
    #[serde(default)]
    pub scroll_delta_y: f32,
    /// Window/safe-area state recorded with each replay input.  The VM uses
    /// this before layout, so a resize follows the same responsive branch on
    /// Native and Web.
    #[serde(default)]
    pub viewport: Option<UiViewport>,
    /// Semantic DOM/UI events. The engine validates IDs and transitions, but
    /// never receives a host layout tree or pixel hit-test result.
    #[serde(default)]
    pub intents: Vec<UiIntent>,
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
            scroll_delta_y: 0.0,
            viewport: None,
            intents: Vec::new(),
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
            scroll_delta_y: 0.0,
            viewport: None,
            intents: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_viewport(mut self, viewport: UiViewport) -> Self {
        self.viewport = Some(viewport);
        self
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
