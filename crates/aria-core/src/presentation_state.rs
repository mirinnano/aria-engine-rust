//! Deterministic presentation inputs and the small amount of semantic
//! navigation state that belongs in a save.
//!
//! This module deliberately has no layout primitives, style tokens, focus
//! keys, or hit-test geometry. Those belong to the React/Tauri presentation
//! package and are reconstructed from the semantic view model on each host.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// A semantic confirmation survives save/load because it changes what the
/// next affirmative input means.  It deliberately contains no copy, focus,
/// or modal geometry; those are presentation concerns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct PendingConfirmation {
    pub action: String,
    /// A backlog page is a stable replay target, not a serialized VM image.
    /// It is populated only for the "resume this page" confirmation.
    pub resume_id: Option<String>,
}

/// Schema 7 represented a confirmation as a bare action string.  Accept that
/// wire shape only long enough to reach the VM's schema gate, so an old save
/// receives the explicit `UnsupportedSnapshot(7)` diagnostic instead of a
/// misleading JSON type error.
impl<'de> Deserialize<'de> for PendingConfirmation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum WireConfirmation {
            Current {
                #[serde(default)]
                action: String,
                #[serde(default)]
                resume_id: Option<String>,
            },
            PreviousSchemaAction(String),
        }

        match WireConfirmation::deserialize(deserializer)? {
            WireConfirmation::Current { action, resume_id } => Ok(Self { action, resume_id }),
            WireConfirmation::PreviousSchemaAction(action) => Ok(Self {
                action,
                resume_id: None,
            }),
        }
    }
}

/// Viewport data travels in replay input so Native and Web render a scene
/// against the same observed window, scale factor, and safe-area values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiViewport {
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub safe_area: UiInsets,
}

impl Default for UiViewport {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            scale_factor: 1.0,
            safe_area: UiInsets::default(),
        }
    }
}

impl UiViewport {
    #[must_use]
    pub fn logical_width(self) -> f32 {
        self.width.max(1) as f32 / self.scale_factor.max(0.1)
    }

    #[must_use]
    pub fn logical_height(self) -> f32 {
        self.height.max(1) as f32 / self.scale_factor.max(0.1)
    }

    #[must_use]
    pub fn content_width(self) -> f32 {
        (self.logical_width() - self.safe_area.left - self.safe_area.right).max(1.0)
    }

    #[must_use]
    pub fn content_height(self) -> f32 {
        (self.logical_height() - self.safe_area.top - self.safe_area.bottom).max(1.0)
    }

    #[must_use]
    pub fn valid(self) -> bool {
        self.width > 0
            && self.height > 0
            && self.scale_factor.is_finite()
            && (0.1..=16.0).contains(&self.scale_factor)
            && self.safe_area.is_valid()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl UiInsets {
    #[must_use]
    pub fn is_valid(self) -> bool {
        [self.top, self.right, self.bottom, self.left]
            .into_iter()
            .all(|value| value.is_finite() && value >= 0.0)
    }
}

/// Persisted semantic presentation state. DOM focus, hover, drag state,
/// slider geometry, and visual tokens intentionally never enter a save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiRuntimeState {
    pub route: String,
    /// Routes below the visible overlay. Closing a nested save/settings sheet
    /// returns to its invoking menu instead of always snapping to dialogue.
    #[serde(default)]
    pub route_stack: Vec<String>,
    /// Semantic list positions are saved; actual DOM scroll and virtualization
    /// remain frontend responsibilities.
    #[serde(default)]
    pub scroll_offsets: BTreeMap<String, f32>,
    /// A destructive rmenu operation awaiting acknowledgement. This is
    /// semantic state, unlike dialog geometry or focus, and therefore remains
    /// valid when a save is resumed on another host.
    #[serde(default)]
    pub confirmation: Option<PendingConfirmation>,
    /// The selected CG while the gallery is in its full-screen reading mode.
    /// Keeping this semantic selection makes a save made from the gallery
    /// reopen to the same image without serializing any presentation state.
    #[serde(default)]
    pub gallery_viewer: Option<String>,
    /// Whether the current interlude is taking its first, longer hold. This
    /// is presentation semantics rather than DOM animation state, so a save
    /// made during the blank-to-text beat can reopen with the same rhythm.
    #[serde(default)]
    pub interlude_first_visit: bool,
    /// Replayed but never serialized: a fresh host owns its viewport after a
    /// load and submits the current value with the next input snapshot.
    #[serde(skip)]
    pub viewport: UiViewport,
}

impl Default for UiRuntimeState {
    fn default() -> Self {
        Self {
            route: "dialogue".to_owned(),
            route_stack: Vec::new(),
            scroll_offsets: BTreeMap::new(),
            confirmation: None,
            gallery_viewer: None,
            interlude_first_visit: false,
            viewport: UiViewport::default(),
        }
    }
}
