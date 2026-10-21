mod game;

use runtime::FrameClock;
use runtime::scene::Scene;
use runtime::{Camera, CameraController};
use runtime::{InputState, Key};

use crate::game::RobotGame;
use engine_logger::ResultLogExt;
use renderer::{WindowRenderer, vulkan::CameraUniform};
use runtime::assets::AssetManager;
use runtime::scene_io;
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

#[derive(Default)]
struct GameApp {
    // These external GPU resources must be released before the renderer/device.
    scene: Scene,
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
}

impl GameApp {
    fn update(&mut self, delta_seconds: f32) {
        if self.project_name.is_some() {
            return;
        }

        // Reload wins if both commands arrive in the same frame, so a save
        // cannot overwrite disk edits that the user intended to reload.
        if self.input.just_pressed(Key::F9) {
            if let Err(error) = self.reload_scene() {
                engine_logger::error!("Failed to reload scene: {error}");
            }
            self.clock.reset();
            return;
        }
        if self.input.just_pressed(Key::F5) {
            if let Err(error) = self.save_scene() {
                engine_logger::error!("Failed to save scene: {error}");
            }
            self.clock.reset();
            return;
        }

        self.camera_controller
            .update(&mut self.camera, &self.input, delta_seconds);

        if let Some(game) = self.game.as_mut() {
            game.update(&mut self.scene, &self.input, delta_seconds);
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
        let loaded = scene_io::load(&scene_path, &asset_root, &renderer)?;
        let game = RobotGame::from_entities(&loaded.entity_ids)?;

        // Install only after every fallible initialization step has succeeded.
        self.scene = loaded.scene;
        self.asset_manager = Some(loaded.assets);
        self.camera = loaded.camera;
        self.game = Some(game);
        self.asset_root = Some(asset_root);
        self.scene_path = Some(scene_path);
        self.renderer = Some(renderer);
        self.window = Some(window);
        self.clock.reset();
        engine_logger::component_health(
            "renderer",
            "HEALTHY",
            false,
            "Game rendering resources initialized",
        );
        Ok(())
    }

    fn save_scene(&self) -> Result<(), String> {
        let path = self
            .scene_path
            .as_deref()
            .ok_or("Scene path is not initialized")?;
        scene_io::save(path, &self.scene, &self.camera)?;
        engine_logger::info!("Saved scene: {}", path.display());
        Ok(())
    }

    fn reload_scene(&mut self) -> Result<(), String> {
        let path = self
            .scene_path
            .as_deref()
            .ok_or("Scene path is not initialized")?;
        let asset_root = self
            .asset_root
            .as_deref()
            .ok_or("Asset root is not initialized")?;
        let renderer = self
            .renderer
            .as_ref()
            .ok_or("Renderer is not initialized")?;

        let loaded = scene_io::load(path, asset_root, renderer)?;
        let game = RobotGame::from_entities(&loaded.entity_ids)?;

        // Drawing can still reference the old meshes and textures. Finish all
        // GPU work before assignments release the old scene, then its cache.
        renderer.wait_idle()?;
        self.scene = loaded.scene;
        self.asset_manager = Some(loaded.assets);
        self.camera = loaded.camera;
        self.game = Some(game);
        engine_logger::info!("Reloaded scene: {}", path.display());
        Ok(())
    }

    fn render(&mut self) -> Result<(), String> {
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

        let objects = self.scene.render_objects();

        renderer.draw(&camera_uniform, &objects)?;
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
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                self.set_mouse_capture(true);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };

                let key = match code {
                    KeyCode::KeyW => Key::W,
                    KeyCode::KeyA => Key::A,
                    KeyCode::KeyS => Key::S,
                    KeyCode::KeyD => Key::D,
                    KeyCode::Space => Key::Space,
                    KeyCode::Escape => Key::Escape,
                    KeyCode::ArrowUp => Key::ArrowUp,
                    KeyCode::ArrowDown => Key::ArrowDown,
                    KeyCode::ArrowLeft => Key::ArrowLeft,
                    KeyCode::ArrowRight => Key::ArrowRight,
                    KeyCode::Tab => Key::Tab,
                    KeyCode::F5 => Key::F5,
                    KeyCode::F9 => Key::F9,
                    _ => return,
                };

                match event.state {
                    ElementState::Pressed => self.input.press(key),
                    ElementState::Released => self.input.release(key),
                }

                if key == Key::Escape && event.state == ElementState::Pressed {
                    self.set_mouse_capture(false);
                }
            }
            WindowEvent::Focused(false) => {
                self.set_mouse_capture(false);
                self.input.reset();
                self.clock.reset();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if size.width == 0 || size.height == 0 {
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

                self.update(delta_seconds);
                self.input.end_frame();

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
