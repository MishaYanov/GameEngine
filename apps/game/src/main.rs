use runtime::scene::{EntityId, Scene};

use engine_logger::ResultLogExt;
use renderer::{vulkan::CameraUniform, WindowRenderer};
use std::{path::PathBuf, sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
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
    start_time: Option<Instant>,
    last_heartbeat: Option<Instant>,
    project_name: Option<String>,
}

impl GameApp {
    fn update(&mut self, elapsed: f32) {
        if self.project_name.is_some() {
            return;
        }
        if let Some(id) = self.left_robot {
            if let Some(left) = self.scene.get_mut(id) {
                left.transform_mut().rotation = [elapsed * 0.6, elapsed * 0.9, 0.0];
            }
        }
        if let Some(id) = self.right_robot {
            if let Some(right) = self.scene.get_mut(id) {
                right.transform_mut().rotation = [elapsed * -0.5, elapsed * 0.75, elapsed * 0.2];
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
        self.start_time = Some(Instant::now());
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
        let camera = CameraUniform::perspective(
            [0.0, 0.0, 3.0],
            [0.0; 3],
            [0.0, 1.0, 0.0],
            60.0_f32.to_radians(),
            width as f32 / height as f32,
            0.1,
            100.0,
        );

        let objects = self.scene.render_objects();

        renderer.draw(&camera, &objects)?;
        Ok(())
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
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if size.width > 0 && size.height > 0 {
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
                    return;
                }
                let elapsed = self
                    .start_time
                    .as_ref()
                    .map(|start| start.elapsed().as_secs_f32())
                    .unwrap_or(0.0);
                self.update(elapsed);
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
