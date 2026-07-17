use std::collections::BTreeSet;

use aria_core::{InputAction, InputSnapshot, PointerSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawControl {
    KeyUp,
    KeyDown,
    KeyLeft,
    KeyRight,
    KeyEnter,
    KeySpace,
    KeyEscape,
    KeyMenu,
    KeyControl,
    MousePrimary,
    TouchPrimary,
    GamepadDpadUp,
    GamepadDpadDown,
    GamepadDpadLeft,
    GamepadDpadRight,
    GamepadSouth,
    GamepadEast,
    GamepadStart,
    GamepadRightShoulder,
    SteamNavigateUp,
    SteamNavigateDown,
    SteamNavigateLeft,
    SteamNavigateRight,
    SteamConfirm,
    SteamCancel,
    SteamMenu,
    SteamAdvance,
    SteamSkip,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RawInputEvent {
    Press { control: RawControl },
    Release { control: RawControl },
    PointerMoved { x: f32, y: f32 },
}

#[derive(Debug, Default)]
pub struct InputNormalizer {
    held_controls: BTreeSet<RawControl>,
    pressed_actions: BTreeSet<InputAction>,
    held_actions: BTreeSet<InputAction>,
    pointer_x: f32,
    pointer_y: f32,
    pointer_present: bool,
    pointer_pressed: bool,
    pointer_held: bool,
}

impl InputNormalizer {
    pub fn push(&mut self, event: RawInputEvent) {
        match event {
            RawInputEvent::PointerMoved { x, y } => {
                self.pointer_x = x;
                self.pointer_y = y;
                self.pointer_present = true;
                self.pointer_held = self.held_controls.contains(&RawControl::MousePrimary)
                    || self.held_controls.contains(&RawControl::TouchPrimary);
            }
            RawInputEvent::Press { control } => {
                if self.held_controls.insert(control) {
                    if let Some(action) = semantic_action(control) {
                        self.pressed_actions.insert(action);
                        self.held_actions.insert(action);
                    }
                    if matches!(control, RawControl::MousePrimary | RawControl::TouchPrimary) {
                        // A Winit mouse button event has no coordinates. Do
                        // not manufacture a (0, 0) click before the first
                        // CursorMoved event: that can activate the wrong
                        // choice on one platform but not another. A location
                        // less click still has the expected advance meaning.
                        if self.pointer_present {
                            self.pointer_pressed = true;
                            self.pointer_held = true;
                        } else {
                            self.pressed_actions.insert(InputAction::Advance);
                        }
                    }
                }
            }
            RawInputEvent::Release { control } => {
                self.held_controls.remove(&control);
                if let Some(action) = semantic_action(control) {
                    // Several physical controls can intentionally share one
                    // semantic action (Enter/A/Steam Confirm). Releasing one
                    // must not clear the action while another mapped control
                    // remains held; otherwise input order differs by host.
                    if !self
                        .held_controls
                        .iter()
                        .any(|held| semantic_action(*held) == Some(action))
                    {
                        self.held_actions.remove(&action);
                    }
                }
                if matches!(control, RawControl::MousePrimary | RawControl::TouchPrimary) {
                    self.pointer_held = self.held_controls.contains(&RawControl::MousePrimary)
                        || self.held_controls.contains(&RawControl::TouchPrimary);
                }
            }
        }
    }

    #[must_use]
    pub fn snapshot(&mut self, sequence: u64, delta_ms: u32) -> InputSnapshot {
        let pointer = self.pointer_present.then_some(PointerSnapshot {
            x: self.pointer_x,
            y: self.pointer_y,
            primary_pressed: self.pointer_pressed,
            primary_held: self.pointer_held,
        });
        self.pointer_pressed = false;
        InputSnapshot {
            sequence,
            delta_ms,
            pressed: std::mem::take(&mut self.pressed_actions),
            held: self.held_actions.clone(),
            pointer,
        }
    }

    /// Drops held state when the window loses focus. Physical release events
    /// are not guaranteed while another application owns the input device.
    pub fn reset_after_focus_loss(&mut self) {
        self.held_controls.clear();
        self.held_actions.clear();
        self.pressed_actions.clear();
        self.pointer_pressed = false;
        self.pointer_held = false;
    }
}

fn semantic_action(control: RawControl) -> Option<InputAction> {
    Some(match control {
        RawControl::KeyUp | RawControl::GamepadDpadUp | RawControl::SteamNavigateUp => {
            InputAction::NavigateUp
        }
        RawControl::KeyDown | RawControl::GamepadDpadDown | RawControl::SteamNavigateDown => {
            InputAction::NavigateDown
        }
        RawControl::KeyLeft | RawControl::GamepadDpadLeft | RawControl::SteamNavigateLeft => {
            InputAction::NavigateLeft
        }
        RawControl::KeyRight | RawControl::GamepadDpadRight | RawControl::SteamNavigateRight => {
            InputAction::NavigateRight
        }
        RawControl::KeyEnter | RawControl::GamepadSouth | RawControl::SteamConfirm => {
            InputAction::Confirm
        }
        RawControl::KeyEscape | RawControl::GamepadEast | RawControl::SteamCancel => {
            InputAction::Cancel
        }
        RawControl::KeyMenu | RawControl::GamepadStart | RawControl::SteamMenu => InputAction::Menu,
        RawControl::KeySpace | RawControl::SteamAdvance => InputAction::Advance,
        RawControl::KeyControl | RawControl::GamepadRightShoulder | RawControl::SteamSkip => {
            InputAction::Skip
        }
        RawControl::MousePrimary | RawControl::TouchPrimary => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_gamepad_and_steam_input_share_semantic_actions() {
        for control in [
            RawControl::KeyEnter,
            RawControl::GamepadSouth,
            RawControl::SteamConfirm,
        ] {
            let mut normalizer = InputNormalizer::default();
            normalizer.push(RawInputEvent::Press { control });
            let snapshot = normalizer.snapshot(1, 16);
            assert_eq!(snapshot.pressed, BTreeSet::from([InputAction::Confirm]));
        }
    }

    #[test]
    fn a_press_is_edge_triggered_but_skip_remains_held() {
        let mut normalizer = InputNormalizer::default();
        normalizer.push(RawInputEvent::Press {
            control: RawControl::GamepadRightShoulder,
        });
        let first = normalizer.snapshot(1, 16);
        let second = normalizer.snapshot(2, 16);
        assert!(first.is_pressed(InputAction::Skip));
        assert!(!second.is_pressed(InputAction::Skip));
        assert!(second.is_held(InputAction::Skip));
    }

    #[test]
    fn mouse_press_without_a_known_position_never_fabricates_origin_coordinates() {
        let mut normalizer = InputNormalizer::default();
        normalizer.push(RawInputEvent::Press {
            control: RawControl::MousePrimary,
        });
        let snapshot = normalizer.snapshot(1, 16);
        assert!(snapshot.pointer.is_none());
        assert!(snapshot.is_pressed(InputAction::Advance));
    }

    #[test]
    fn focus_loss_releases_skip_and_pointer_state() {
        let mut normalizer = InputNormalizer::default();
        normalizer.push(RawInputEvent::PointerMoved { x: 32.0, y: 48.0 });
        normalizer.push(RawInputEvent::Press {
            control: RawControl::GamepadRightShoulder,
        });
        normalizer.push(RawInputEvent::Press {
            control: RawControl::MousePrimary,
        });
        normalizer.reset_after_focus_loss();
        let snapshot = normalizer.snapshot(1, 16);
        assert!(!snapshot.is_held(InputAction::Skip));
        assert!(!snapshot.pointer.unwrap().primary_held);
    }

    #[test]
    fn releasing_one_of_two_confirm_controls_keeps_confirm_held() {
        let mut normalizer = InputNormalizer::default();
        normalizer.push(RawInputEvent::Press {
            control: RawControl::KeyEnter,
        });
        normalizer.push(RawInputEvent::Press {
            control: RawControl::GamepadSouth,
        });
        normalizer.push(RawInputEvent::Release {
            control: RawControl::KeyEnter,
        });
        let snapshot = normalizer.snapshot(1, 16);
        assert!(snapshot.is_held(InputAction::Confirm));
    }

    #[test]
    fn releasing_mouse_does_not_clear_a_simultaneous_touch_hold() {
        let mut normalizer = InputNormalizer::default();
        normalizer.push(RawInputEvent::PointerMoved { x: 10.0, y: 20.0 });
        normalizer.push(RawInputEvent::Press {
            control: RawControl::MousePrimary,
        });
        normalizer.push(RawInputEvent::Press {
            control: RawControl::TouchPrimary,
        });
        normalizer.push(RawInputEvent::Release {
            control: RawControl::MousePrimary,
        });
        assert!(normalizer.snapshot(1, 16).pointer.unwrap().primary_held);
    }
}
