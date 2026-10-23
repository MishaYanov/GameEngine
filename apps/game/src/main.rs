mod editor_panel;
mod editor_tools;
mod game;
mod gizmo_overlay;
mod selection_overlay;

use editor_panel::PanelView;
use editor_tools::EditorTools;
use runtime::FrameClock;
use runtime::scene::Scene;
use runtime::{Camera, CameraController};
use runtime::{InputState, Key};
use runtime::{SceneEditor, Transform, model_instance::ModelInstance};
use selection_overlay::SelectionOverlay;

use crate::game::RobotGame;
use engine_logger::ResultLogExt;
use renderer::{WindowRenderer, vulkan::CameraUniform};
use runtime::assets::AssetManager;
use runtime::scene_io;
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

#[derive(Clone, Copy)]
enum SceneCommand {
    Reload,
    Save,
    TogglePlay,
    DeleteSelected,
    DuplicateSelected,
    SpawnRobot,
}

struct AuthoredScene {
    scene: Scene,
    camera: Camera,
}

#[derive(Default)]
struct GameApp {
    // These external GPU resources must be released before the renderer/device.
    scene: Scene,
    authored: Option<AuthoredScene>,
    editor: SceneEditor,
    tools: EditorTools,
    selection_overlay: Option<SelectionOverlay>,
    spawn_template: Option<ModelInstance>,
    asset_manager: Option<AssetManager>,
    renderer: Option<WindowRenderer>,
    window: Option<Arc<Window>>,
    clock: FrameClock,
    input: InputState,
    camera: Camera,
    camera_controller: CameraController,
    game: Option<RobotGame>,
    mouse_captured: bool,
    last_heartbeat: Option<Instant>,
    project_name: Option<String>,
    asset_root: Option<PathBuf>,
    scene_path: Option<PathBuf>,
    cursor_position: Option<[f64; 2]>,
    last_window_title: String,
}

impl GameApp {
    fn update(&mut self, delta_seconds: f32) {
        if self.project_name.is_some() {
            return;
        }
        if self.handle_scene_commands() {
            return;
        }
        if self.tools.panel.edit.is_some() || self.tools.drag.is_some() {
            return;
        }

        self.camera_controller
            .update(&mut self.camera, &self.input, delta_seconds);

        if self.authored.is_some() {
            let Some(game) = self.game.as_mut() else {
                return;
            };
            let previous_paused = game.animation_paused();

            if let Err(error) = game.update(&mut self.scene, &self.input, delta_seconds) {
                engine_logger::error!("Failed to update game: {error}");
            }

            if game.animation_paused() != previous_paused {
                self.update_selection_title();
            }
        } else {
            // Structural undo can retire GPU-backed models. This is an explicit
            // key command, so the wait is never paid by ordinary movement frames.
            let control =
                self.input.is_held(Key::ControlLeft) || self.input.is_held(Key::ControlRight);
            if control
                && (self.input.just_pressed(Key::Z) || self.input.just_pressed(Key::Y))
                && let Err(error) = self.wait_for_gpu()
            {
                engine_logger::error!("Cannot restore scene edit: {error}");
                return;
            }
            let previous_selection = self.editor.selected_entity();
            if let Err(error) = self
                .editor
                .update(&mut self.scene, &self.input, delta_seconds)
            {
                engine_logger::error!("Failed to edit scene: {error}");
            }
            if self.editor.selected_entity() != previous_selection {
                self.update_selection_title();
            }
        }
    }

    fn handle_scene_commands(&mut self) -> bool {
        // The first pressed command wins. Reload precedes save to protect disk edits.
        let command = [
            (Key::F9, SceneCommand::Reload),
            (Key::F5, SceneCommand::Save),
            (Key::F6, SceneCommand::TogglePlay),
            (Key::Delete, SceneCommand::DeleteSelected),
            (Key::C, SceneCommand::DuplicateSelected),
            (Key::N, SceneCommand::SpawnRobot),
        ]
            .into_iter()
            .find_map(|(key, command)| self.input.just_pressed(key).then_some(command));
        let Some(command) = command else {
            return false;
        };

        let (action, result) = self.execute_scene_command(command);
        if let Err(error) = result {
            engine_logger::error!("Failed to {action}: {error}");
        }

        if matches!(
            command,
            SceneCommand::DeleteSelected
                | SceneCommand::DuplicateSelected
                | SceneCommand::SpawnRobot
        ) {
            self.update_selection_title();
        }
        self.clock.reset();

        true
    }

    fn execute_scene_command(
        &mut self,
        command: SceneCommand,
    ) -> (&'static str, Result<(), String>) {
        match command {
            SceneCommand::Reload => ("reload scene", self.reload_scene()),
            SceneCommand::Save => ("save scene", self.save_scene()),
            SceneCommand::TogglePlay => ("change Edit/Play mode", self.toggle_play()),
            SceneCommand::DeleteSelected => {
                ("delete selected entity", self.apply_entity_command(command))
            }
            SceneCommand::DuplicateSelected => (
                "duplicate selected entity",
                self.apply_entity_command(command),
            ),
            SceneCommand::SpawnRobot => ("spawn robot", self.apply_entity_command(command)),
        }
    }

    fn update_selection_title(&mut self) {
        if self.project_name.is_some() {
            return;
        }
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let selected_name = self
            .editor
            .selected_entity()
            .and_then(|id| self.scene.name(id))
            .unwrap_or("None");

        let mode = match (self.authored.is_some(), self.game.as_ref()) {
            (true, Some(game)) if game.animation_paused() => "Play paused",
            (true, _) => "Play",
            (false, _) => "Edit",
        };

        let title = format!(
            "Game Engine{} | {mode} | Selected: {selected_name} | F6 Edit/Play",
            if self.is_dirty() { " *" } else { "" }
        );
        if title != self.last_window_title {
            window.set_title(&title);
            self.last_window_title = title;
        }
    }

    fn initialize(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(self.project_name.as_deref().unwrap_or("Game Engine"))
                        .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
                )
                .map_err(|e| e.to_string())?,
        );

        let size = window.inner_size();
        let renderer = WindowRenderer::new(Arc::clone(&window), size.width, size.height)?;
        let asset_root = if self.project_name.is_some() {
            std::env::current_exe()
                .map_err(|e| e.to_string())?
                .parent()
                .ok_or("executable has no parent")?
                .join("assets")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
        };

        let scene_path = asset_root.join("scenes/robots.json");
        let mut loaded = scene_io::load(&scene_path, &asset_root, &renderer)?;
        let editor = Self::prepare_editor(&mut loaded)?;
        let overlay = if self.project_name.is_none() {
            Some(SelectionOverlay::new(&renderer, &mut loaded.assets)?)
        } else {
            None
        };
        let mut tools = EditorTools {
            saved: Some(loaded.scene.to_data(&loaded.camera)?),
            ..EditorTools::default()
        };
        if self.project_name.is_none() {
            tools.panel.initialize(&renderer, &mut loaded.assets)?;
            tools.gizmo = Some(gizmo_overlay::GizmoOverlay::new(
                &renderer,
                &mut loaded.assets,
            )?);
            tools.models = runtime::assets::catalog::scan_models(&asset_root)?;
        }

        // Install only after every fallible initialization step has succeeded.
        self.scene = loaded.scene;
        self.asset_manager = Some(loaded.assets);
        self.camera = loaded.camera;
        self.editor = editor;
        self.tools = tools;
        self.selection_overlay = overlay;
        self.game = None;
        self.asset_root = Some(asset_root);
        self.scene_path = Some(scene_path);
        self.renderer = Some(renderer);
        self.window = Some(window);
        self.clock.reset();
        self.update_selection_title();
        engine_logger::component_health(
            "renderer",
            "HEALTHY",
            false,
            "Game rendering resources initialized",
        );
        Ok(())
    }

    fn save_scene(&mut self) -> Result<(), String> {
        let path = self
            .scene_path
            .as_deref()
            .ok_or("Scene path is not initialized")?;
        let (scene, camera) = match self.authored.as_ref() {
            Some(authored) => (&authored.scene, &authored.camera),
            None => (&self.scene, &self.camera),
        };
        scene_io::save(path, scene, camera)?;
        self.tools.saved = Some(scene.to_data(camera)?);
        engine_logger::info!("Saved scene: {}", path.display());
        self.update_selection_title();
        Ok(())
    }

    fn wait_for_gpu(&self) -> Result<(), String> {
        if let Some(renderer) = self.renderer.as_ref() {
            renderer.wait_idle()?;
        }
        Ok(())
    }

    fn prepare_editor(loaded: &mut scene_io::LoadedScene) -> Result<SceneEditor, String> {
        if loaded.source_schema_version == 1
            && let [left_robot, right_robot, ..] = loaded.entity_ids.as_slice()
        {
            loaded.scene.set_binding("robot_a", *left_robot)?;
            loaded.scene.set_binding("robot_b", *right_robot)?;
        }
        Ok(SceneEditor::from_scene(&loaded.scene))
    }

    fn apply_entity_command(&mut self, command: SceneCommand) -> Result<(), String> {
        if self.authored.is_some() {
            return Err("Stop Play mode with F6 before editing entities".to_string());
        }
        self.wait_for_gpu()?;
        match command {
            SceneCommand::DeleteSelected => self.editor.delete_selected(&mut self.scene),
            SceneCommand::DuplicateSelected => {
                self.editor.duplicate_selected(&mut self.scene).map(|_| ())
            }
            SceneCommand::SpawnRobot => self.spawn_robot(),
            _ => Err("Expected an entity editing command".to_string()),
        }
    }

    fn spawn_robot(&mut self) -> Result<(), String> {
        if self.spawn_template.is_none() {
            let existing = self
                .scene
                .entity_ids()
                .find(|id| self.scene.model_path(*id) == Some("props/robot.glb"))
                .and_then(|id| self.scene.get(id))
                .cloned();
            let mut instance = match existing {
                Some(instance) => instance,
                None => {
                    let renderer = self
                        .renderer
                        .as_ref()
                        .ok_or("Renderer is not initialized")?;
                    let assets = self
                        .asset_manager
                        .as_mut()
                        .ok_or("Assets are not initialized")?;
                    let model = assets
                        .load_model(renderer, "props/robot.glb")
                        .map_err(|error| error.to_string())?;
                    ModelInstance::new(Arc::new(model), Transform::default())
                }
            };
            *instance.transform_mut() = Transform {
                scale: [0.7; 3],
                ..Transform::default()
            };
            self.spawn_template = Some(instance);
        }
        self.editor.spawn(
            &mut self.scene,
            "Robot",
            "props/robot.glb",
            self.spawn_template
                .as_ref()
                .ok_or("Robot template is missing")?
                .clone(),
        )?;
        Ok(())
    }

    fn toggle_play(&mut self) -> Result<(), String> {
        if self.authored.is_some() {
            self.stop_play()
        } else {
            self.start_play()
        }
    }

    fn start_play(&mut self) -> Result<(), String> {
        if self.authored.is_some() {
            return Ok(());
        }
        self.cancel_drag();
        let play_scene = self.scene.clone();
        let game =
            if play_scene.binding("robot_a").is_some() && play_scene.binding("robot_b").is_some() {
                Some(RobotGame::from_scene(&play_scene)?)
            } else {
                None
            };
        let authored_scene = std::mem::replace(&mut self.scene, play_scene);
        self.authored = Some(AuthoredScene {
            scene: authored_scene,
            camera: self.camera.clone(),
        });
        self.game = game;
        self.finish_mode_change();
        Ok(())
    }

    fn stop_play(&mut self) -> Result<(), String> {
        self.wait_for_gpu()?;
        let Some(authored) = self.authored.take() else {
            return Ok(());
        };
        self.scene = authored.scene;
        self.camera = authored.camera;
        self.game = None;
        self.finish_mode_change();
        Ok(())
    }

    fn finish_mode_change(&mut self) {
        self.tools.panel.edit = None;
        self.set_mouse_capture(false);
        self.input.suppress_held();
        self.editor.end_gesture();
        self.clock.reset();
        self.update_selection_title();
    }

    fn reload_scene(&mut self) -> Result<(), String> {
        let path = self
            .scene_path
            .clone()
            .ok_or("Scene path is not initialized")?;
        let asset_root = self
            .asset_root
            .as_deref()
            .ok_or("Asset root is not initialized")?;
        let renderer = self
            .renderer
            .as_ref()
            .ok_or("Renderer is not initialized")?;

        let mut loaded = scene_io::load(&path, asset_root, renderer)?;
        let editor = Self::prepare_editor(&mut loaded)?;
        let saved = loaded.scene.to_data(&loaded.camera)?;

        // Drawing can still reference the old meshes and textures. Finish all
        // GPU work before assignments release the old scene, then its cache.
        renderer.wait_idle()?;
        self.scene = loaded.scene;
        self.authored = None;
        self.spawn_template = None;
        self.tools.templates.clear();
        self.tools.drag = None;
        self.tools.saved = Some(saved);
        self.editor = editor;
        self.asset_manager = Some(loaded.assets);
        self.camera = loaded.camera;
        self.game = None;
        self.finish_mode_change();
        engine_logger::info!("Reloaded scene: {}", path.display());
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
        let dirty = self.is_dirty();
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        let Some([width, height]) = renderer.prepare()? else {
            return Ok(());
        };
        let camera_uniform = CameraUniform::perspective(
            self.camera.position,
            self.camera.target(),
            [0.0, 1.0, 0.0],
            self.camera.vertical_fov,
            width as f32 / height as f32,
            self.camera.near,
            self.camera.far,
        );

        let frustum = if self.tools.culling {
            Some(runtime::visibility::Frustum::from_view_projection(
                glam::Mat4::from_cols_array(&camera_uniform.view_projection),
            )?)
        } else {
            None
        };
        let (mut objects, mut stats) = self.scene.render_visible(frustum.as_ref());
        if self.authored.is_none()
            && self.project_name.is_none()
            && let Some(object) = self
            .selection_overlay
            .as_ref()
            .and_then(|overlay| overlay.object(&self.scene, &self.editor))
        {
            stats.draw_calls += 1;
            stats.triangles += object.mesh().index_count() as usize / 3;
            objects.push(object);
        }

        if self.project_name.is_none() {
            if self.authored.is_none()
                && let Some(transform) = self
                .editor
                .selected_entity()
                .and_then(|id| self.scene.get(id))
                .map(|instance| *instance.transform())
                && let Some(gizmo) = self.tools.gizmo.as_mut()
            {
                gizmo.prepare(
                    renderer,
                    &self.camera,
                    [width, height],
                    transform,
                    self.tools.panel.mode,
                    self.tools.drag.as_ref().map(|drag| drag.axis),
                )?;
                let object = gizmo.object();
                stats.draw_calls += 1;
                stats.triangles += object.mesh().index_count() as usize / 3;
                objects.push(object);
            }
            let scale = self
                .window
                .as_ref()
                .map_or(1.0, |window| window.scale_factor());
            // The panel is one batched draw. Include its previous geometry in
            // displayed totals; the current text has the same quad count.
            if let Some(panel) = self.tools.panel.object() {
                stats.draw_calls += 1;
                stats.triangles += panel.mesh().index_count() as usize / 3;
            }
            self.tools.panel.prepare(
                renderer,
                [width, height],
                scale,
                PanelView {
                    scene: &self.scene,
                    editor: &self.editor,
                    models: &self.tools.models,
                    playing: self.authored.is_some(),
                    dirty,
                    stats,
                    frame_ms: self.tools.metrics.frame_ms,
                    cpu_ms: self.tools.metrics.cpu_ms,
                    culling: self.tools.culling,
                },
            )?;
            if let Some(panel) = self.tools.panel.object() {
                objects.push(panel);
            }
        }
        self.tools.stats = stats;

        renderer.draw(&camera_uniform, &objects)?;
        Ok(())
    }

    fn pick_entity(&mut self) -> Result<(), String> {
        if self.authored.is_some() || self.project_name.is_some() || self.mouse_captured {
            return Ok(());
        }
        let Some(pixel) = self.cursor_position else {
            return Ok(());
        };
        let window = self.window.as_ref().ok_or("Window is not initialized")?;
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let Some(extent) = self
            .renderer
            .as_mut()
            .ok_or("Renderer is not initialized")?
            .prepare()?
        else {
            return Ok(());
        };
        let pixel = [
            pixel[0] * extent[0] as f64 / size.width as f64,
            pixel[1] * extent[1] as f64 / size.height as f64,
        ];
        self.editor.pick(&self.scene, &self.camera, pixel, extent)?;
        self.update_selection_title();
        Ok(())
    }

    fn set_mouse_capture(&mut self, capture: bool) {
        let Some(window) = self.window.as_ref() else {
            self.mouse_captured = false;
            return;
        };

        if capture {
            if self.mouse_captured || self.project_name.is_some() || !window.has_focus() {
                return;
            }

            let result = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));

            match result {
                Ok(()) => {
                    window.set_cursor_visible(false);
                    self.mouse_captured = true;
                }
                Err(error) => {
                    engine_logger::error!("Failed to capture mouse: {error}");
                }
            }
        } else {
            // Always stop mouse look and restore visibility, even if releasing fails.
            self.mouse_captured = false;
            if let Err(error) = window.set_cursor_grab(CursorGrabMode::None) {
                engine_logger::error!("Failed to release mouse: {error}");
            }
            window.set_cursor_visible(true);
        }
    }
}

impl ApplicationHandler for GameApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if self
            .initialize(event_loop)
            .map_err(std::io::Error::other)
            .log_error_for("game::renderer", "initialize game")
            .is_err()
        {
            event_loop.exit();
            return;
        }
        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_position = Some([position.x, position.y]);
                if let Err(error) = self.preview_drag() {
                    self.report_editor_error(error);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.cursor_position = None;
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Err(error) = self.left_press() {
                    self.report_editor_error(error);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if let Err(error) = self.finish_drag() {
                    self.report_editor_error(error);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(pixel) = self.cursor_position {
                    let scale = self
                        .window
                        .as_ref()
                        .map_or(1.0, |window| window.scale_factor());
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(position) => position.y as f32 / 26.0,
                    };
                    self.tools.panel.scroll(pixel, scale, lines);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                self.cancel_drag();
                self.tools.panel.edit = None;
                self.set_mouse_capture(true);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                if event.state == ElementState::Pressed && self.editor_key(code, &event) {
                    return;
                }

                let key = match code {
                    KeyCode::KeyW => Key::W,
                    KeyCode::KeyA => Key::A,
                    KeyCode::KeyS => Key::S,
                    KeyCode::KeyD => Key::D,
                    KeyCode::KeyQ => Key::Q,
                    KeyCode::KeyE => Key::E,
                    KeyCode::KeyR => Key::R,
                    KeyCode::KeyF => Key::F,
                    KeyCode::KeyN => Key::N,
                    KeyCode::KeyC => Key::C,
                    KeyCode::KeyZ => Key::Z,
                    KeyCode::KeyY => Key::Y,
                    KeyCode::ControlLeft => Key::ControlLeft,
                    KeyCode::ControlRight => Key::ControlRight,
                    KeyCode::Delete => Key::Delete,
                    KeyCode::Space => Key::Space,
                    KeyCode::Escape => Key::Escape,
                    KeyCode::ArrowUp => Key::ArrowUp,
                    KeyCode::ArrowDown => Key::ArrowDown,
                    KeyCode::ArrowLeft => Key::ArrowLeft,
                    KeyCode::ArrowRight => Key::ArrowRight,
                    KeyCode::Tab => Key::Tab,
                    KeyCode::F5 => Key::F5,
                    KeyCode::F6 => Key::F6,
                    KeyCode::F9 => Key::F9,
                    _ => return,
                };

                match event.state {
                    ElementState::Pressed if !event.repeat => self.input.press(key),
                    ElementState::Pressed => {}
                    ElementState::Released => self.input.release(key),
                }

                if key == Key::Escape && event.state == ElementState::Pressed {
                    self.set_mouse_capture(false);
                }
            }
            WindowEvent::Focused(false) => {
                self.cancel_drag();
                self.tools.panel.edit = None;
                self.set_mouse_capture(false);
                self.input.reset();
                self.clock.reset();
            }
            WindowEvent::Resized(size) => {
                self.cancel_drag();
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if size.width == 0 || size.height == 0 {
                    self.cancel_drag();
                    self.set_mouse_capture(false);
                    self.clock.reset();
                } else {
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self
                    .last_heartbeat
                    .is_none_or(|last| last.elapsed().as_secs() >= 5)
                {
                    engine_logger::heartbeat("game");
                    self.last_heartbeat = Some(Instant::now());
                }
                let size = self.window.as_ref().unwrap().inner_size();

                if size.width == 0 || size.height == 0 {
                    self.clock.reset();
                    return;
                }

                let delta_seconds = self.clock.tick(Instant::now()).as_secs_f32();
                let update_start = Instant::now();
                self.update(delta_seconds);
                self.tools
                    .metrics
                    .record(update_start, update_start.elapsed().as_secs_f32());
                self.input.end_frame();
                self.update_selection_title();

                if self
                    .render()
                    .map_err(std::io::Error::other)
                    .log_error_for("game::renderer", "render frame")
                    .is_err()
                {
                    event_loop.exit();
                    return;
                }
                self.window.as_ref().unwrap().request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if !self.mouse_captured
            || self
                .window
                .as_ref()
                .is_none_or(|window| !window.has_focus())
        {
            return;
        }

        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            self.camera_controller
                .mouse_motion(&mut self.camera, dx, dy);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.set_mouse_capture(false);
    }
}

impl Drop for GameApp {
    fn drop(&mut self) {
        // Wait before automatic field destruction releases scene meshes/textures.
        if let Some(renderer) = self.renderer.as_ref() {
            let _ = renderer
                .wait_idle()
                .map_err(std::io::Error::other)
                .log_error_for("game::renderer", "wait before releasing scene");
        }
    }
}

fn main() {
    let _logging =
        engine_logger::init_application("game").expect("failed to initialize game logger");

    // Importing happens once in GameApp::resumed.
    let event_loop = EventLoop::new().expect("failed to create event loop");

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = GameApp::default();

    app.project_name = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read_to_string(exe.parent()?.join("project-name.txt")).ok());

    engine_logger::component_health("game", "HEALTHY", true, "Game event loop started");

    event_loop
        .run_app(&mut app)
        .expect("game event loop failed");

    engine_logger::component_health("game", "STOPPED", false, "Game event loop stopped");
}

#[cfg(test)]
#[path = "tests_app_scene_io.rs"]
mod tests;
