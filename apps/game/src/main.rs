use runtime::scene::{EntityId, Scene};
use runtime::FrameClock;
use runtime::{InputState, Key};
use runtime::Camera;

use engine_logger::ResultLogExt;
use renderer::{vulkan::CameraUniform, WindowRenderer};
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{CursorGrabMode, Window, WindowId},
};
use runtime::assets::AssetManager;
use runtime::model_instance::ModelInstance;
use runtime::transform::Transform;

#[derive(Default)]
struct GameApp {
    // These external GPU resources must be released before the renderer/device.
    scene: Scene,
    asset_manager: Option<AssetManager>,
    renderer: Option<WindowRenderer>,
    window: Option<Arc<Window>>,
    left_robot: Option<EntityId>,
    right_robot: Option<EntityId>,
    clock: FrameClock,
    input: InputState,
    camera: Camera,
    mouse_captured: bool,
    last_heartbeat: Option<Instant>,
    project_name: Option<String>,
}

impl GameApp {
    fn update(&mut self, delta_seconds: f32) {
        if self.project_name.is_some() {
            return;
        }

        let mut direction = [0.0_f32; 2];
        let mut camera_direction = [0.0_f32; 2];

        if self.input.is_held(Key::W) {
            direction[1] += 1.0;
        }

        if self.input.is_held(Key::S) {
            direction[1] -= 1.0;
        }

        if self.input.is_held(Key::A) {
            direction[0] -= 1.0;
        }

        if self.input.is_held(Key::D) {
            direction[0] += 1.0;
        }

        if self.input.is_held(Key::ArrowUp) {
            camera_direction[1] += 1.0;
        }

        if self.input.is_held(Key::ArrowDown) {
            camera_direction[1] -= 1.0;
        }

        if self.input.is_held(Key::ArrowLeft) {
            camera_direction[0] -= 1.0;
        }

        if self.input.is_held(Key::ArrowRight) {
            camera_direction[0] += 1.0;
        }

        let length =
            (direction[0] * direction[0] + direction[1] * direction[1]).sqrt();

        if length > 0.0 {
            direction[0] /= length;
            direction[1] /= length;
        }

        let camera_length = (
            camera_direction[0] * camera_direction[0]
                + camera_direction[1] * camera_direction[1]
        ).sqrt();

        if camera_length > 0.0 {
            camera_direction[0] /= camera_length;
            camera_direction[1] /= camera_length;
        }

        let forward = self.camera.forward();
        let right = self.camera.right();
        let camera_speed = 2.0;
        let distance = camera_speed * delta_seconds;

        for axis in 0..3 {
            self.camera.position[axis] +=
                (right[axis] * camera_direction[0]
                    + forward[axis] * camera_direction[1])
                    * distance;
        }

        let move_speed = 1.0; // World units per second.
        let distance = move_speed * delta_seconds;

        if let Some(id) = self.left_robot {
            if let Some(left) = self.scene.get_mut(id) {
                let transform = &mut left.transform_mut();

                transform.position[0] += direction[0] * distance;
                transform.position[1] += direction[1] * distance;

                transform.rotation[0] += 0.6 * delta_seconds;
                transform.rotation[1] += 0.9 * delta_seconds;
            }
        }
        if let Some(id) = self.right_robot {
            if let Some(right) = self.scene.get_mut(id) {
                let rotation = &mut right.transform_mut().rotation;
                rotation[0] += delta_seconds * -0.5;
                rotation[1] += delta_seconds * 0.75;
                rotation[2] += delta_seconds * 0.2;
            }
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
        let mut assets = AssetManager::new(asset_root).map_err(|e| e.to_string())?;
        let model = Arc::new(
            assets
                .load_model(&renderer, "props/robot.glb")
                .map_err(|e| format!("Failed to load robot model: {e}"))?,
        );
        self.left_robot = Some(self.scene.spawn(ModelInstance::new(
            Arc::clone(&model),
            Transform {
                position: [-0.8, 0.0, 0.0],
                scale: [0.7; 3],
                ..Transform::default()
            },
        )));
        self.right_robot = Some(self.scene.spawn(ModelInstance::new(
            Arc::clone(&model),
            Transform {
                position: [0.8, 0.0, 0.0],
                scale: [0.7; 3],
                ..Transform::default()
            },
        )));
        self.asset_manager = Some(assets);
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

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        if !self.mouse_captured
            || self.window.as_ref().is_none_or(|window| !window.has_focus())
        {
            return;
        }

        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event {
            let sensitivity = 0.002_f32;
            // Mouse deltas are displacement, so they must not be multiplied by frame time.
            self.camera
                .rotate(dx as f32 * sensitivity, -dy as f32 * sensitivity);
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.set_mouse_capture(false);
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
            WindowEvent::KeyboardInput {event, ..} => {
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
                    _ => return,
                };

                match event.state {
                    ElementState::Pressed => self.input.press(key),
                    ElementState::Released => self.input.release(key),
                }

                if key == Key::Escape && event.state == ElementState::Pressed {
                    self.set_mouse_capture(false);
                }
            },
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
