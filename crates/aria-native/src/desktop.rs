//! Winit event translation kept outside `aria-core`.

use aria_render::ViewportTransform;
use winit::event::{ElementState, MouseButton, TouchPhase, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::input::{InputNormalizer, RawControl, RawInputEvent};

/// Converts physical Winit coordinates and keys into the stable input
/// vocabulary consumed by the native and web runtimes.
#[derive(Debug)]
pub struct WinitInputAdapter {
    normalizer: InputNormalizer,
    viewport: ViewportTransform,
}

impl WinitInputAdapter {
    #[must_use]
    pub fn new(viewport: ViewportTransform) -> Self {
        Self {
            normalizer: InputNormalizer::default(),
            viewport,
        }
    }

    pub fn set_viewport(&mut self, viewport: ViewportTransform) {
        self.viewport = viewport;
    }

    pub fn push_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::KeyboardInput { event, .. } if !event.repeat => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if let Some(control) = keyboard_control(code) {
                    self.normalizer.push(button_event(event.state, control));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.push_pointer(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self
                .normalizer
                .push(button_event(*state, RawControl::MousePrimary)),
            WindowEvent::Touch(touch) => {
                self.push_pointer(touch.location.x as f32, touch.location.y as f32);
                match touch.phase {
                    TouchPhase::Started => self.normalizer.push(RawInputEvent::Press {
                        control: RawControl::TouchPrimary,
                    }),
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.normalizer.push(RawInputEvent::Release {
                            control: RawControl::TouchPrimary,
                        });
                    }
                    TouchPhase::Moved => {}
                }
            }
            _ => {}
        }
    }

    /// Feeds a controller event from the native gilrs/Steam seam into the same
    /// normalizer used by Winit. Keeping this method value-only keeps the
    /// Windows/Linux action contract identical without exposing either SDK to
    /// `aria-core`.
    pub fn push_controller_event(&mut self, event: RawInputEvent) {
        self.normalizer.push(event);
    }

    #[must_use]
    pub fn snapshot(&mut self, sequence: u64, delta_ms: u32) -> aria_core::InputSnapshot {
        self.normalizer.snapshot(sequence, delta_ms)
    }

    /// Clears held controls after an OS focus transition. This keeps Skip and
    /// pointer presses from becoming platform-dependent stuck inputs.
    pub fn reset_after_focus_loss(&mut self) {
        self.normalizer.reset_after_focus_loss();
    }

    /// Routes a semantic click from an accessibility client through the same
    /// pointer path used by mouse and touch input. Keeping this here avoids
    /// teaching the Core about AccessKit or a platform-specific node ID.
    pub fn accessibility_click(&mut self, logical_x: f32, logical_y: f32) {
        self.normalizer.push(RawInputEvent::PointerMoved {
            x: logical_x,
            y: logical_y,
        });
        self.normalizer.push(RawInputEvent::Press {
            control: RawControl::MousePrimary,
        });
        self.normalizer.push(RawInputEvent::Release {
            control: RawControl::MousePrimary,
        });
    }

    /// Updates hover/focus selection from an accessibility focus request.
    pub fn accessibility_hover(&mut self, logical_x: f32, logical_y: f32) {
        self.normalizer.push(RawInputEvent::PointerMoved {
            x: logical_x,
            y: logical_y,
        });
    }

    fn push_pointer(&mut self, physical_x: f32, physical_y: f32) {
        let (x, y) = self.viewport.physical_to_logical(physical_x, physical_y);
        self.normalizer.push(RawInputEvent::PointerMoved { x, y });
    }
}

fn button_event(state: ElementState, control: RawControl) -> RawInputEvent {
    match state {
        ElementState::Pressed => RawInputEvent::Press { control },
        ElementState::Released => RawInputEvent::Release { control },
    }
}

fn keyboard_control(code: KeyCode) -> Option<RawControl> {
    Some(match code {
        KeyCode::ArrowUp => RawControl::KeyUp,
        KeyCode::ArrowDown => RawControl::KeyDown,
        KeyCode::ArrowLeft => RawControl::KeyLeft,
        KeyCode::ArrowRight => RawControl::KeyRight,
        KeyCode::Enter | KeyCode::NumpadEnter => RawControl::KeyEnter,
        KeyCode::Space => RawControl::KeySpace,
        KeyCode::Escape => RawControl::KeyEscape,
        KeyCode::ContextMenu => RawControl::KeyMenu,
        KeyCode::ControlLeft | KeyCode::ControlRight => RawControl::KeyControl,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use aria_core::InputAction;
    use aria_core::protocol::{LogicalSize, Rect};
    use aria_render::SafeAreaInsets;
    use winit::dpi::PhysicalPosition;
    use winit::event::DeviceId;

    use super::*;

    fn adapter() -> WinitInputAdapter {
        WinitInputAdapter::new(ViewportTransform::fit(
            LogicalSize {
                width: 1280,
                height: 720,
            },
            1920,
            1080,
            1.0,
            SafeAreaInsets::default(),
        ))
    }

    #[test]
    fn cursor_coordinates_enter_the_logical_viewport() {
        let mut adapter = adapter();
        adapter.push_window_event(&WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(960.0, 540.0),
        });
        let pointer = adapter.snapshot(1, 16).pointer.unwrap();
        assert!((pointer.x - 640.0).abs() < 0.001);
        assert!((pointer.y - 360.0).abs() < 0.001);
    }

    #[test]
    fn keyboard_mapping_uses_semantic_actions() {
        assert_eq!(keyboard_control(KeyCode::Enter), Some(RawControl::KeyEnter));
        let mut normalizer = InputNormalizer::default();
        normalizer.push(button_event(ElementState::Pressed, RawControl::KeyEnter));
        assert!(normalizer.snapshot(1, 16).is_pressed(InputAction::Confirm));
    }

    #[test]
    fn controller_events_use_the_same_semantic_snapshot() {
        let mut adapter = adapter();
        adapter.push_controller_event(RawInputEvent::Press {
            control: RawControl::SteamConfirm,
        });
        assert!(adapter.snapshot(1, 16).is_pressed(InputAction::Confirm));
    }

    #[test]
    fn viewport_can_be_replaced_after_dpi_or_resize() {
        let mut adapter = adapter();
        adapter.set_viewport(ViewportTransform {
            scale: 2.0,
            offset_x: 10.0,
            offset_y: 20.0,
            logical_safe_area: Rect {
                x: 0.0,
                y: 0.0,
                width: 1280.0,
                height: 720.0,
            },
            minimum_target_size: 44.0,
        });
        adapter.push_pointer(210.0, 220.0);
        let pointer = adapter.snapshot(1, 16).pointer.unwrap();
        assert_eq!((pointer.x, pointer.y), (100.0, 100.0));
    }

    #[test]
    fn accessibility_click_uses_the_same_pointer_snapshot_as_mouse_input() {
        let mut adapter = adapter();
        adapter.accessibility_click(480.0, 240.0);
        let pointer = adapter.snapshot(1, 16).pointer.unwrap();
        assert_eq!((pointer.x, pointer.y), (480.0, 240.0));
        assert!(pointer.primary_pressed);
        assert!(!pointer.primary_held);
    }
}
