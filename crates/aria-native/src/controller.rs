//! Native gamepad polling and semantic input translation.
//!
//! `gilrs` is intentionally kept in this adapter.  The Core sees only the
//! value-only [`RawInputEvent`] stream, so the same bytecode and VM behaviour
//! is used on Windows and Linux (and Steam Input's standard-controller
//! emulation can be consumed without teaching the language about a controller
//! SDK).

use std::collections::{BTreeMap, BTreeSet};

use gilrs::{Axis, Button, EventType, Gilrs};

use crate::input::{RawControl, RawInputEvent};

const PRESS_THRESHOLD: f32 = 0.5;
const RELEASE_THRESHOLD: f32 = 0.35;

/// Polls host gamepads and emits the stable Aria raw-control vocabulary.
#[derive(Debug)]
pub struct GilrsController {
    gilrs: Gilrs,
    state: ControllerState,
}

impl GilrsController {
    /// Creates a controller context.  Failure is non-fatal for a visual novel
    /// (a machine can have no gamepad backend), so the Player treats the error
    /// as an unavailable optional input device.
    pub fn new() -> Result<Self, String> {
        Gilrs::new()
            .map(|gilrs| Self {
                gilrs,
                state: ControllerState::default(),
            })
            .map_err(|error| error.to_string())
    }

    /// Drains all pending host events without blocking the render loop.
    #[must_use]
    pub fn poll_events(&mut self) -> Vec<RawInputEvent> {
        let mut output = Vec::new();
        while let Some(event) = self.gilrs.next_event() {
            self.state
                .handle_event(event.id.into(), event.event, &mut output);
        }
        output
    }

    /// Clears device state after focus loss.  Operating systems do not
    /// guarantee release events while another application owns the window.
    pub fn reset_after_focus_loss(&mut self) {
        self.state.clear();
    }
}

#[derive(Debug, Default)]
struct ControllerState {
    /// A raw control can be held by several physical sources at once (two
    /// pads, a button and a stick direction, or a pad and Steam Input).  Only
    /// the first press and final release are forwarded to the normalizer.
    held_sources: BTreeMap<RawControl, BTreeSet<SourceId>>,
    axis_directions: BTreeMap<(usize, u16), i8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SourceId {
    gamepad: usize,
    code: u16,
}

impl ControllerState {
    fn handle_event(&mut self, gamepad: usize, event: EventType, output: &mut Vec<RawInputEvent>) {
        match event {
            EventType::ButtonPressed(button, _) | EventType::ButtonRepeated(button, _) => {
                if let Some(control) = button_control(button) {
                    let source = SourceId {
                        gamepad,
                        code: button_source_code(button),
                    };
                    push_if_some(output, self.press(source, control));
                }
            }
            EventType::ButtonReleased(button, _) => {
                if let Some(control) = button_control(button) {
                    let source = SourceId {
                        gamepad,
                        code: button_source_code(button),
                    };
                    push_if_some(output, self.release(source, control));
                }
            }
            EventType::ButtonChanged(button, value, _) => {
                if let Some(control) = button_control(button) {
                    let source = SourceId {
                        gamepad,
                        code: button_source_code(button),
                    };
                    let held = self.source_is_held(source, control);
                    if !held && value >= PRESS_THRESHOLD {
                        push_if_some(output, self.press(source, control));
                    } else if held && value <= RELEASE_THRESHOLD {
                        push_if_some(output, self.release(source, control));
                    }
                }
            }
            EventType::AxisChanged(axis, value, _) => {
                self.handle_axis(gamepad, axis, value, output);
            }
            EventType::Disconnected => output.extend(self.clear_gamepad(gamepad)),
            EventType::Connected | EventType::Dropped | EventType::ForceFeedbackEffectCompleted => {
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }

    fn handle_axis(
        &mut self,
        gamepad: usize,
        axis: Axis,
        value: f32,
        output: &mut Vec<RawInputEvent>,
    ) {
        let Some((negative_control, positive_control)) = axis_controls(axis) else {
            return;
        };
        let axis_key = (gamepad, axis as u16);
        let previous = self.axis_directions.get(&axis_key).copied().unwrap_or(0);
        let next = axis_direction(previous, value);
        if previous == next {
            return;
        }

        if let Some(control) = axis_direction_control(negative_control, positive_control, previous)
        {
            let source = SourceId {
                gamepad,
                code: axis_source_code(axis, previous),
            };
            push_if_some(output, self.release(source, control));
        }
        if let Some(control) = axis_direction_control(negative_control, positive_control, next) {
            let source = SourceId {
                gamepad,
                code: axis_source_code(axis, next),
            };
            push_if_some(output, self.press(source, control));
        }

        if next == 0 {
            self.axis_directions.remove(&axis_key);
        } else {
            self.axis_directions.insert(axis_key, next);
        }
    }

    fn press(&mut self, source: SourceId, control: RawControl) -> Option<RawInputEvent> {
        let sources = self.held_sources.entry(control).or_default();
        if !sources.insert(source) || sources.len() != 1 {
            return None;
        }
        Some(RawInputEvent::Press { control })
    }

    fn release(&mut self, source: SourceId, control: RawControl) -> Option<RawInputEvent> {
        let sources = self.held_sources.get_mut(&control)?;
        if !sources.remove(&source) || !sources.is_empty() {
            return None;
        }
        self.held_sources.remove(&control);
        Some(RawInputEvent::Release { control })
    }

    fn source_is_held(&self, source: SourceId, control: RawControl) -> bool {
        self.held_sources
            .get(&control)
            .is_some_and(|sources| sources.contains(&source))
    }

    fn clear_gamepad(&mut self, gamepad: usize) -> Vec<RawInputEvent> {
        let mut released = Vec::new();
        let controls = self.held_sources.keys().copied().collect::<Vec<_>>();
        for control in controls {
            let mut became_unheld = false;
            if let Some(sources) = self.held_sources.get_mut(&control) {
                sources.retain(|source| source.gamepad != gamepad);
                became_unheld = sources.is_empty();
            }
            if became_unheld {
                self.held_sources.remove(&control);
                released.push(RawInputEvent::Release { control });
            }
        }
        self.axis_directions
            .retain(|(source_gamepad, _), _| *source_gamepad != gamepad);
        released
    }

    fn clear(&mut self) {
        self.held_sources.clear();
        self.axis_directions.clear();
    }
}

fn push_if_some(output: &mut Vec<RawInputEvent>, event: Option<RawInputEvent>) {
    if let Some(event) = event {
        output.push(event);
    }
}

fn button_control(button: Button) -> Option<RawControl> {
    Some(match button {
        Button::South => RawControl::GamepadSouth,
        Button::East => RawControl::GamepadEast,
        Button::North => RawControl::GamepadNorth,
        Button::Start => RawControl::GamepadStart,
        Button::DPadUp => RawControl::GamepadDpadUp,
        Button::DPadDown => RawControl::GamepadDpadDown,
        Button::DPadLeft => RawControl::GamepadDpadLeft,
        Button::DPadRight => RawControl::GamepadDpadRight,
        Button::RightTrigger | Button::RightTrigger2 => RawControl::GamepadRightShoulder,
        _ => return None,
    })
}

fn axis_controls(axis: Axis) -> Option<(RawControl, RawControl)> {
    Some(match axis {
        Axis::LeftStickX | Axis::RightStickX | Axis::DPadX => {
            (RawControl::GamepadDpadLeft, RawControl::GamepadDpadRight)
        }
        Axis::LeftStickY | Axis::RightStickY | Axis::DPadY => {
            (RawControl::GamepadDpadUp, RawControl::GamepadDpadDown)
        }
        _ => return None,
    })
}

fn axis_direction(previous: i8, value: f32) -> i8 {
    match previous {
        -1 if value <= -RELEASE_THRESHOLD => -1,
        1 if value >= RELEASE_THRESHOLD => 1,
        _ if value <= -PRESS_THRESHOLD => -1,
        _ if value >= PRESS_THRESHOLD => 1,
        _ => 0,
    }
}

fn axis_direction_control(
    negative_control: RawControl,
    positive_control: RawControl,
    direction: i8,
) -> Option<RawControl> {
    match direction {
        -1 => Some(negative_control),
        1 => Some(positive_control),
        _ => None,
    }
}

fn button_source_code(button: Button) -> u16 {
    0x4000 | button as u16
}

fn axis_source_code(axis: Axis, direction: i8) -> u16 {
    let sign = u16::from(direction > 0);
    0x8000 | ((axis as u16) << 1) | sign
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_buttons_map_to_semantic_controls() {
        assert_eq!(
            button_control(Button::South),
            Some(RawControl::GamepadSouth)
        );
        assert_eq!(button_control(Button::East), Some(RawControl::GamepadEast));
        assert_eq!(
            button_control(Button::North),
            Some(RawControl::GamepadNorth)
        );
        assert_eq!(
            button_control(Button::Start),
            Some(RawControl::GamepadStart)
        );
        assert_eq!(
            button_control(Button::RightTrigger),
            Some(RawControl::GamepadRightShoulder)
        );
    }

    #[test]
    fn gilrs_button_events_become_normalizer_events() {
        let mut state = ControllerState::default();
        let mut output = Vec::new();
        let code = Button::South.to_nec().expect("standard button has a code");
        state.handle_event(
            0,
            EventType::ButtonPressed(Button::South, code),
            &mut output,
        );
        state.handle_event(
            0,
            EventType::ButtonReleased(Button::South, code),
            &mut output,
        );
        assert_eq!(
            output,
            vec![
                RawInputEvent::Press {
                    control: RawControl::GamepadSouth,
                },
                RawInputEvent::Release {
                    control: RawControl::GamepadSouth,
                },
            ]
        );
    }

    #[test]
    fn stick_threshold_has_hysteresis() {
        assert_eq!(axis_direction(0, 0.49), 0);
        assert_eq!(axis_direction(0, 0.5), 1);
        assert_eq!(axis_direction(1, 0.36), 1);
        assert_eq!(axis_direction(1, 0.34), 0);
        assert_eq!(axis_direction(-1, -0.36), -1);
        assert_eq!(axis_direction(-1, -0.34), 0);
    }

    #[test]
    fn two_gamepads_share_a_control_until_the_last_release() {
        let mut state = ControllerState::default();
        let control = RawControl::GamepadSouth;
        let first = SourceId {
            gamepad: 0,
            code: button_source_code(Button::South),
        };
        let second = SourceId {
            gamepad: 1,
            code: button_source_code(Button::South),
        };
        assert!(state.press(first, control).is_some());
        assert!(state.press(second, control).is_none());
        assert!(state.release(first, control).is_none());
        assert!(state.release(second, control).is_some());
    }

    #[test]
    fn disconnect_releases_only_the_disconnected_pad() {
        let mut state = ControllerState::default();
        let first = SourceId {
            gamepad: 0,
            code: button_source_code(Button::South),
        };
        let second = SourceId {
            gamepad: 1,
            code: button_source_code(Button::South),
        };
        state.press(first, RawControl::GamepadSouth);
        state.press(second, RawControl::GamepadSouth);
        assert!(state.clear_gamepad(0).is_empty());
        assert!(state.source_is_held(second, RawControl::GamepadSouth));
        assert_eq!(
            state.clear_gamepad(1),
            vec![RawInputEvent::Release {
                control: RawControl::GamepadSouth,
            }]
        );
    }
}
