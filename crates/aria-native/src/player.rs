//! Windowed Native Player for the V3 protocol boundary.
//!
//! The Player owns Winit, wgpu, Kira, filesystem saves, and AccessKit.  None
//! of these types cross into `aria-core`: the VM receives deterministic input
//! snapshots and returns render/audio/runtime commands only.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use accesskit::{Action, NodeId};
use accesskit_winit::WindowEvent as AccessKitWindowEvent;
use accesskit_winit::{Adapter as AccessKitAdapter, Event as AccessKitEvent};
use aria_core::protocol::{AudioCommand, LogicalSize, RuntimeCommand, StepOutput, UiNode};
use aria_core::{
    CompiledProgram, InputSnapshot, SaveEnvelopeError, SaveEnvelopeV3, Vm, VmSnapshot,
};
use aria_render::{BundledFont, RenderSurfaceSize, WgpuRenderer};
use thiserror::Error;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize as WindowLogicalSize;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::accessibility::AccessTreeBuilder;
use crate::assets::NativeAssetStore;
use crate::audio::KiraAudioAdapter;
use crate::controller::GilrsController;
use crate::desktop::WinitInputAdapter;
use crate::storage::{AtomicSaveStore, SaveStoreError};

const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// The adapter contract clamps one delivered frame to the same 250 ms upper
/// bound used by the PWA. Suspends and debugger pauses therefore cannot make
/// Windows and Linux advance a delay, transition, or typewriter by seconds.
const MAX_FRAME_DELTA_MS: u32 = 250;

/// Values needed to launch a graphical desktop Player.
pub struct NativePlayerConfig {
    pub title: String,
    pub program: CompiledProgram,
    pub logical_size: LogicalSize,
    /// Parent directory; `AtomicSaveStore` adds `save_namespace` beneath it.
    pub save_root: PathBuf,
    pub save_namespace: String,
    /// Ordered, exact logical font assets from `aria.toml`/bundle metadata.
    /// No platform font discovery is permitted by the Native Player.
    pub font_assets: Vec<String>,
    pub assets: NativeAssetStore,
}

impl std::fmt::Debug for NativePlayerConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativePlayerConfig")
            .field("title", &self.title)
            .field("program_game_id", &self.program.game_id)
            .field("logical_size", &self.logical_size)
            .field("save_root", &self.save_root)
            .field("save_namespace", &self.save_namespace)
            .field("font_assets", &self.font_assets)
            .field("assets", &self.assets)
            .finish()
    }
}

/// Errors surfaced before or during a Native Player session.
#[derive(Debug, Error)]
pub enum NativePlayerError {
    #[error("cannot create the native event loop: {0}")]
    EventLoop(String),
    #[error("cannot create the native window: {0}")]
    Window(String),
    #[error("cannot create a wgpu surface: {0}")]
    Surface(String),
    #[error("cannot select a compatible graphics adapter: {0}")]
    Adapter(String),
    #[error("cannot create a wgpu device: {0}")]
    Device(String),
    #[error(transparent)]
    Vm(#[from] aria_core::VmError),
    #[error(transparent)]
    Save(#[from] SaveStoreError),
    #[error(transparent)]
    SaveEnvelope(#[from] SaveEnvelopeError),
    #[error("native renderer failed: {0}")]
    Renderer(String),
}

/// Returns the per-user location used by a Native Player unless `ARIA_SAVE_DIR`
/// overrides it. The namespace itself is validated by `AtomicSaveStore`.
#[must_use]
pub fn default_save_root() -> PathBuf {
    if let Some(override_root) = std::env::var_os("ARIA_SAVE_DIR") {
        return PathBuf::from(override_root);
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = std::env::var_os("APPDATA") {
            return PathBuf::from(app_data).join("AriaEngine");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("AriaEngine");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(data_home).join("aria-engine");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("aria-engine");
        }
    }
    PathBuf::from("saves-v3")
}

/// Starts the real Winit/wgpu Native Player. This is intentionally separate
/// from the terminal/replay runner used by automated deterministic tests.
pub fn run_desktop(config: NativePlayerConfig) -> Result<(), NativePlayerError> {
    let event_loop = EventLoop::<PlayerEvent>::with_user_event()
        .build()
        .map_err(|error| NativePlayerError::EventLoop(error.to_string()))?;
    let proxy = event_loop.create_proxy();
    let mut application = NativeApplication {
        pending_config: Some(config),
        proxy,
        window: None,
        failure: None,
    };
    event_loop
        .run_app(&mut application)
        .map_err(|error| NativePlayerError::EventLoop(error.to_string()))?;
    if let Some(error) = application.failure {
        Err(error)
    } else {
        Ok(())
    }
}

#[derive(Debug)]
enum PlayerEvent {
    Access(AccessKitEvent),
}

impl From<AccessKitEvent> for PlayerEvent {
    fn from(event: AccessKitEvent) -> Self {
        Self::Access(event)
    }
}

struct NativeApplication {
    pending_config: Option<NativePlayerConfig>,
    proxy: EventLoopProxy<PlayerEvent>,
    window: Option<WindowState>,
    failure: Option<NativePlayerError>,
}

struct WindowState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    renderer: WgpuRenderer,
    input: WinitInputAdapter,
    controller: Option<GilrsController>,
    vm: Vm,
    save_store: AtomicSaveStore,
    assets: NativeAssetStore,
    audio: Option<KiraAudioAdapter>,
    output: StepOutput,
    access_adapter: AccessKitAdapter,
    access_tree: AccessTreeBuilder,
    sequence: u64,
    last_tick: Instant,
    warned: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameDisposition {
    Continue,
    Quit,
}

impl NativeApplication {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), NativePlayerError> {
        let config = self
            .pending_config
            .take()
            .expect("the Native Player creates one initial window");
        let NativePlayerConfig {
            title,
            program,
            logical_size,
            save_root,
            save_namespace,
            font_assets,
            assets,
        } = config;
        let title = if title.trim().is_empty() {
            "AriaEngine".to_owned()
        } else {
            title
        };
        let attributes = Window::default_attributes()
            .with_title(title)
            .with_inner_size(WindowLogicalSize::new(
                f64::from(logical_size.width),
                f64::from(logical_size.height),
            ))
            .with_min_inner_size(WindowLogicalSize::new(320.0, 180.0))
            .with_visible(false);
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|error| NativePlayerError::Window(error.to_string()))?,
        );
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(Arc::clone(&window))
            .map_err(|error| NativePlayerError::Surface(error.to_string()))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|error| NativePlayerError::Adapter(error.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("AriaEngine Native Player"),
            ..Default::default()
        }))
        .map_err(|error| NativePlayerError::Device(error.to_string()))?;
        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| {
                NativePlayerError::Surface(
                    "the selected adapter cannot present to this window".to_owned(),
                )
            })?;
        // Prefer stable vsync semantics across Windows, macOS, and Linux while
        // retaining the platform-selected format and alpha mode.
        surface_config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &surface_config);
        let mut assets = assets;
        let bundled_fonts = font_assets
            .into_iter()
            .map(|logical_path| {
                let bytes = assets.read(&logical_path).map_err(|message| {
                    NativePlayerError::Renderer(format!(
                        "cannot load bundled font '{logical_path}': {message}"
                    ))
                })?;
                Ok(BundledFont {
                    logical_path,
                    bytes,
                })
            })
            .collect::<Result<Vec<_>, NativePlayerError>>()?;
        let renderer =
            WgpuRenderer::new_with_fonts(device, queue, surface_config.format, bundled_fonts)
                .map_err(|error| NativePlayerError::Renderer(error.to_string()))?;

        let mut vm = Vm::new(program, logical_size)?;
        let output = vm.step(&InputSnapshot::idle(1, 0))?;
        let viewport = renderer.viewport_transform(
            &output.render,
            RenderSurfaceSize::new(size.width.max(1), size.height.max(1), window.scale_factor()),
        );
        let save_store = AtomicSaveStore::new(save_root, save_namespace)?;
        let audio = match KiraAudioAdapter::new(".") {
            Ok(audio) => Some(audio),
            Err(error) => {
                eprintln!(
                    "warning: native audio is unavailable; continuing without sound: {error}"
                );
                None
            }
        };
        let controller = match GilrsController::new() {
            Ok(controller) => Some(controller),
            Err(error) => {
                eprintln!(
                    "warning: native gamepad input is unavailable; continuing without it: {error}"
                );
                None
            }
        };
        let access_adapter =
            AccessKitAdapter::with_event_loop_proxy(event_loop, &window, self.proxy.clone());
        let mut state = WindowState {
            window: Arc::clone(&window),
            surface,
            surface_config,
            renderer,
            input: WinitInputAdapter::new(viewport),
            controller,
            vm,
            save_store,
            assets,
            audio,
            output,
            access_adapter,
            access_tree: AccessTreeBuilder,
            sequence: 1,
            last_tick: Instant::now(),
            warned: BTreeSet::new(),
        };
        state.apply_audio_commands()?;
        state.process_runtime_commands()?;
        state.update_accessibility();
        window.set_visible(true);
        window.request_redraw();
        self.window = Some(state);
        Ok(())
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: NativePlayerError) {
        eprintln!("error: {error}");
        self.failure = Some(error);
        event_loop.exit();
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let result = self
            .window
            .as_mut()
            .expect("redraw only arrives for the Player window")
            .step_and_render();
        match result {
            Ok(FrameDisposition::Continue) => {}
            Ok(FrameDisposition::Quit) => event_loop.exit(),
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn save_hotkey(&mut self, event_loop: &ActiveEventLoop, slot: u32) {
        let result = self
            .window
            .as_mut()
            .expect("save hotkeys only arrive for the Player window")
            .save_slot(slot);
        if let Err(error) = result {
            self.fail(event_loop, error);
        }
    }

    fn load_hotkey(&mut self, event_loop: &ActiveEventLoop, slot: u32) {
        let result = self
            .window
            .as_mut()
            .expect("load hotkeys only arrive for the Player window")
            .load_slot(slot);
        if let Err(error) = result {
            self.fail(event_loop, error);
        } else if let Some(state) = &self.window {
            state.window.request_redraw();
        }
    }
}

impl ApplicationHandler<PlayerEvent> for NativeApplication {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && self.pending_config.is_some()
            && let Err(error) = self.create_window(event_loop)
        {
            self.fail(event_loop, error);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.window else {
            return;
        };
        if state.window.id() != window_id {
            return;
        }
        let window = Arc::clone(&state.window);
        state.access_adapter.process_event(&window, &event);

        if let Some(slot) = pressed_system_slot(&event, KeyCode::F5) {
            self.save_hotkey(event_loop, slot);
            return;
        }
        if let Some(slot) = pressed_system_slot(&event, KeyCode::F9) {
            self.load_hotkey(event_loop, slot);
            return;
        }

        state.input.push_window_event(&event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                state.resize(size.width, size.height);
                state.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = state.window.inner_size();
                state.resize(size.width, size.height);
                state.window.request_redraw();
            }
            WindowEvent::Focused(false) => {
                state.input.reset_after_focus_loss();
                if let Some(controller) = &mut state.controller {
                    controller.reset_after_focus_loss();
                }
                state.last_tick = Instant::now();
            }
            WindowEvent::Focused(true) => {
                // Treat focus regain as a new clock epoch. Otherwise a
                // minimised window observes a host-specific wall-clock gap.
                state.last_tick = Instant::now();
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn user_event(&mut self, _: &ActiveEventLoop, event: PlayerEvent) {
        let PlayerEvent::Access(event) = event;
        let Some(state) = &mut self.window else {
            return;
        };
        if state.window.id() != event.window_id {
            return;
        }
        match event.window_event {
            AccessKitWindowEvent::InitialTreeRequested => state.update_accessibility(),
            AccessKitWindowEvent::ActionRequested(request) => {
                state.apply_accessibility_action(request.action, request.target_node);
                state.window.request_redraw();
            }
            AccessKitWindowEvent::AccessibilityDeactivated => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(state) = &self.window else {
            event_loop.exit();
            return;
        };
        if state.output.halted {
            event_loop.set_control_flow(ControlFlow::Wait);
        } else {
            event_loop.set_control_flow(ControlFlow::wait_duration(FRAME_INTERVAL));
            state.window.request_redraw();
        }
    }
}

impl WindowState {
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface
            .configure(self.renderer.device(), &self.surface_config);
        self.input.set_viewport(
            self.renderer
                .viewport_transform(&self.output.render, self.render_surface_size()),
        );
    }

    fn render_surface_size(&self) -> RenderSurfaceSize {
        RenderSurfaceSize::new(
            self.surface_config.width,
            self.surface_config.height,
            self.window.scale_factor(),
        )
    }

    fn step_and_render(&mut self) -> Result<FrameDisposition, NativePlayerError> {
        let now = Instant::now();
        let delta_ms = now
            .saturating_duration_since(self.last_tick)
            .as_millis()
            .min(u128::from(MAX_FRAME_DELTA_MS)) as u32;
        self.last_tick = now;
        self.sequence = self.sequence.saturating_add(1);
        self.poll_controller();
        let input = self.input.snapshot(self.sequence, delta_ms);
        self.output = self.vm.step(&input)?;
        self.apply_audio_commands()?;
        let disposition = self.process_runtime_commands()?;
        self.update_accessibility();
        if disposition == FrameDisposition::Quit {
            return Ok(disposition);
        }

        let (surface_texture, reconfigure_after_present) = match self.surface.get_current_texture()
        {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(FrameDisposition::Continue);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.resize(self.surface_config.width, self.surface_config.height);
                self.window.request_redraw();
                return Ok(FrameDisposition::Continue);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(NativePlayerError::Renderer(
                    "wgpu rejected surface texture acquisition".to_owned(),
                ));
            }
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render(
                &self.output.render,
                &view,
                self.render_surface_size(),
                &mut self.assets,
            )
            .map_err(|error| NativePlayerError::Renderer(error.to_string()))?;
        self.renderer.present(surface_texture);
        if reconfigure_after_present {
            self.resize(self.surface_config.width, self.surface_config.height);
        }
        Ok(if self.output.halted {
            FrameDisposition::Quit
        } else {
            FrameDisposition::Continue
        })
    }

    fn poll_controller(&mut self) {
        let events = self
            .controller
            .as_mut()
            .map_or_else(Vec::new, GilrsController::poll_events);
        for event in events {
            self.input.push_controller_event(event);
        }
    }

    fn apply_audio_commands(&mut self) -> Result<(), NativePlayerError> {
        let (audio, assets, warned) = (&mut self.audio, &mut self.assets, &mut self.warned);
        let Some(audio) = audio.as_mut() else {
            return Ok(());
        };
        for command in &self.output.audio {
            let asset_bytes = match command {
                AudioCommand::Play { asset, .. } => match assets.read(asset) {
                    Ok(bytes) => Some(bytes),
                    Err(error) => {
                        warn_once(
                            warned,
                            format!("audio-asset:{asset}"),
                            format!("cannot load audio asset '{asset}': {error}"),
                        );
                        continue;
                    }
                },
                AudioCommand::Stop { .. } | AudioCommand::SetBusVolume { .. } => None,
            };
            if let Err(error) = audio.apply_bytes(command, asset_bytes) {
                warn_once(
                    warned,
                    format!("audio-command:{command:?}"),
                    format!("native audio command was skipped: {error}"),
                );
            }
        }
        Ok(())
    }

    fn process_runtime_commands(&mut self) -> Result<FrameDisposition, NativePlayerError> {
        let commands = self.output.runtime.clone();
        for command in commands {
            match command {
                RuntimeCommand::Save { slot } => self.save_slot(slot)?,
                RuntimeCommand::Load { slot } => self.load_slot(slot)?,
                RuntimeCommand::Unsupported { name, .. } => warn_once(
                    &mut self.warned,
                    format!("host-command:{name}"),
                    format!("runtime skipped unsupported host command '{name}'"),
                ),
                RuntimeCommand::Quit => return Ok(FrameDisposition::Quit),
                RuntimeCommand::OpenMenu => warn_once(
                    &mut self.warned,
                    "menu-request".to_owned(),
                    "menu action was requested, but this game has no declarative menu scene"
                        .to_owned(),
                ),
            }
        }
        Ok(FrameDisposition::Continue)
    }

    fn save_slot(&mut self, slot: u32) -> Result<(), NativePlayerError> {
        let snapshot = self.vm.snapshot();
        let envelope = SaveEnvelopeV3::new(
            snapshot.game_id.clone(),
            aria_core::ENGINE_VERSION,
            now_unix_ms(),
            &snapshot,
        )?;
        self.save_store.save(slot, &envelope)?;
        eprintln!("saved slot {slot}");
        Ok(())
    }

    fn load_slot(&mut self, slot: u32) -> Result<(), NativePlayerError> {
        let Some(loaded) = self.save_store.load(slot)? else {
            warn_once(
                &mut self.warned,
                format!("missing-save:{slot}"),
                format!("save slot {slot} does not exist"),
            );
            return Ok(());
        };
        let snapshot: VmSnapshot = loaded.envelope.payload_as()?;
        loaded.envelope.validate_for_game(&snapshot.game_id)?;
        if let Some(audio) = &mut self.audio {
            audio.stop_all();
        }
        self.vm.restore(snapshot)?;
        if loaded.recovered_from_previous {
            eprintln!("warning: recovered save slot {slot} from the previous generation");
        }
        eprintln!("loaded slot {slot}");
        Ok(())
    }

    fn update_accessibility(&mut self) {
        let update = self.access_tree.build(&self.output.ui);
        self.access_adapter.update_if_active(|| update);
    }

    fn apply_accessibility_action(&mut self, action: Action, target: NodeId) {
        let Some(node) = self.output.ui.nodes.get(&target.0) else {
            return;
        };
        let (x, y) = node_center(node);
        match action {
            Action::Focus => self.input.accessibility_hover(x, y),
            Action::Click => self.input.accessibility_click(x, y),
            _ => {}
        }
    }
}

fn pressed_system_slot(event: &WindowEvent, key: KeyCode) -> Option<u32> {
    let WindowEvent::KeyboardInput { event, .. } = event else {
        return None;
    };
    if event.state != ElementState::Pressed || event.repeat {
        return None;
    }
    matches!(event.physical_key, PhysicalKey::Code(code) if code == key).then_some(1)
}

fn node_center(node: &UiNode) -> (f32, f32) {
    (
        node.bounds.x + node.bounds.width * 0.5,
        node.bounds.y + node.bounds.height * 0.5,
    )
}

fn warn_once(warned: &mut BTreeSet<String>, key: String, message: String) {
    if warned.insert(key) {
        eprintln!("warning: {message}");
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use aria_core::InputAction;
    use aria_core::protocol::{Rect, UiActivation, UiRole};

    use super::*;

    #[test]
    fn default_save_root_has_a_nonempty_fallback() {
        assert!(!default_save_root().as_os_str().is_empty());
    }

    #[test]
    fn accessibility_target_uses_the_center_of_logical_ui_bounds() {
        let node = UiNode {
            id: 9,
            role: UiRole::Button,
            label: "続ける".to_owned(),
            bounds: Rect {
                x: 100.0,
                y: 50.0,
                width: 200.0,
                height: 40.0,
            },
            focusable: true,
            focused: true,
            activation: Some(UiActivation::Input(InputAction::Confirm)),
            children: Vec::new(),
        };
        assert_eq!(node_center(&node), (200.0, 70.0));
    }
}
