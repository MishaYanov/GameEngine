//! Run with cargo run -p game --example renderer_smoke (requires native Vulkan).
use runtime::{Transform, assets, model_instance::ModelInstance, scene::Scene};

use renderer::{
    DrawStatus, WindowRenderer,
    vulkan::{CameraUniform, ModelPushConstants},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

#[derive(Default)]
struct Smoke {
    // Drop uploaded resources before the renderer.
    scene: Scene,
    assets: Option<assets::AssetManager>,
    renderer: Option<WindowRenderer>,
    window: Option<Arc<Window>>,
    frames: usize,
    started: Option<Instant>,
}

impl ApplicationHandler for Smoke {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        let window = Arc::new(
            events
                .create_window(
                    Window::default_attributes()
                        .with_title("Renderer lifecycle smoke test")
                        .with_visible(false)
                        .with_inner_size(winit::dpi::PhysicalSize::new(320, 240)),
                )
                .unwrap(),
        );
        let size = window.inner_size();
        let renderer = WindowRenderer::new(Arc::clone(&window), size.width, size.height).unwrap();
        let mut assets = assets::AssetManager::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"),
        )
        .unwrap();
        let model = Arc::new(assets.load_model(&renderer, "props/robot.glb").unwrap());
        assert!(!model.parts().is_empty());
        let first_transform = Transform {
            position: [-0.8, 0.0, 0.0],
            rotation: [0.2, 0.3, 0.4],
            scale: [0.7; 3],
        };
        let second_transform = Transform {
            position: [0.8, 0.0, 0.0],
            ..Transform::default()
        };
        let first = self
            .scene
            .spawn(ModelInstance::new(Arc::clone(&model), first_transform));
        let second = self
            .scene
            .spawn(ModelInstance::new(Arc::clone(&model), second_transform));
        {
            let objects = self.scene.render_objects();
            assert_eq!(objects.len(), model.parts().len() * 2);
            for (index, object) in objects.iter().enumerate() {
                let part = &model.parts()[index % model.parts().len()];
                assert!(std::ptr::eq(object.mesh(), part.mesh()));
                assert!(std::ptr::eq(object.material(), part.material()));
                let transform = if index < model.parts().len() {
                    self.scene.instances().next().unwrap().transform()
                } else {
                    self.scene.instances().nth(1).unwrap().transform()
                };
                let expected = ModelPushConstants::from_3d(
                    transform.position,
                    transform.rotation,
                    transform.scale,
                );
                assert_eq!(object.transform().model, expected.model);
            }
        }
        drop(self.scene.remove(first).unwrap());
        self.scene.get_mut(second).unwrap().transform_mut().position = [0.0; 3];
        for object in self.scene.render_objects() {
            assert_eq!(
                object.transform().model,
                ModelPushConstants::identity().model
            );
        }
        self.assets = Some(assets);
        self.renderer = Some(renderer);
        self.window = Some(window);
        self.started = Some(Instant::now());
    }

    fn window_event(&mut self, _events: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::Resized(size) = event {
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.resize(size.width, size.height);
            }
        }
    }

    fn about_to_wait(&mut self, events: &ActiveEventLoop) {
        if self.frames >= 5 {
            return;
        }
        assert!(
            self.started.unwrap().elapsed() < Duration::from_secs(30),
            "smoke test timed out"
        );
        let renderer = self.renderer.as_mut().unwrap();
        let [width, height] = renderer.prepare().unwrap().unwrap();
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
        if renderer.draw(&camera, &objects).unwrap() != DrawStatus::Rendered {
            return;
        }
        drop(objects);
        self.frames += 1;
        if self.frames == 2 {
            renderer.wait_idle().unwrap();
            self.scene = Scene::default();
            self.assets.as_mut().unwrap().clear();
            renderer.resize(0, 0);
            assert_eq!(renderer.prepare().unwrap(), None);
            assert_eq!(renderer.draw(&camera, &[]).unwrap(), DrawStatus::Suspended);
            let _ = self
                .window
                .as_ref()
                .unwrap()
                .request_inner_size(winit::dpi::PhysicalSize::new(640, 360));
            let size = self.window.as_ref().unwrap().inner_size();
            renderer.resize(size.width, size.height);
        }
        if self.frames == 5 {
            assert_eq!([width, height], [640, 360], "resized surface extent");
            renderer.wait_idle().unwrap();
            self.assets = None;
            self.renderer = None;
            self.window = None;
            events.exit();
        }
    }
}

impl Drop for Smoke {
    fn drop(&mut self) {
        if let Some(renderer) = &self.renderer {
            let _ = renderer.wait_idle();
        }
    }
}

fn main() {
    let mut config = engine_logger::Config::new("renderer-smoke");
    config.error_backtraces = false;
    config.console = false;
    let _logging = engine_logger::init(config).unwrap();
    let events = EventLoop::new().unwrap();
    events.set_control_flow(ControlFlow::Poll);
    let mut smoke = Smoke::default();
    events.run_app(&mut smoke).unwrap();
    assert_eq!(smoke.frames, 5);
    let errors: Vec<_> = engine_logger::snapshot(Default::default())
        .entries
        .into_iter()
        .filter(|entry| entry.level == engine_logger::Level::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "native validation logged errors: {errors:?}"
    );
    println!(
        "PASS: scene resource mapping and transforms, textured draws, resource release, zero-size suspension, empty resize/draw, shutdown"
    );
}
