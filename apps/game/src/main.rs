mod assets;

use assets::AssetManager;
use engine_logger::ResultLogExt;

use std::{path::PathBuf, sync::Arc, time::Instant};

use renderer::vulkan::{
    CameraUniform, FrameStatus, ModelPushConstants, RenderObject, VulkanCamera, VulkanDepthBuffer,
    VulkanDevice, VulkanFrame, VulkanInstance, VulkanMaterial, VulkanMaterialPipelines, VulkanMesh,
    VulkanSurface, VulkanSwapchain,
};

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::{Window, WindowId},
};

/// One drawable part; instances borrow these resources instead of uploading copies.
struct ModelPart {
    mesh: Arc<VulkanMesh>,
    material: VulkanMaterial,
}

#[derive(Default)]
struct GameApp {
    frame: Option<VulkanFrame>,
    parts: Vec<ModelPart>,
    pipelines: Option<VulkanMaterialPipelines>,
    camera: Option<VulkanCamera>,
    asset_manager: Option<AssetManager>,
    depth_buffer: Option<VulkanDepthBuffer>,
    swapchain: Option<VulkanSwapchain>,
    device: Option<VulkanDevice>,
    surface: Option<VulkanSurface>,
    instance: Option<VulkanInstance>,
    window: Option<Window>,
    start_time: Option<Instant>,
    swapchain_dirty: bool,
    last_heartbeat: Option<Instant>,
    project_name: Option<String>,
}

impl GameApp {
    fn recreate_swapchain(&mut self) -> Result<bool, String> {
        let size = self
            .window
            .as_ref()
            .ok_or("window is not initialized".to_string())?
            .inner_size();

        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }

        let instance = self
            .instance
            .as_ref()
            .ok_or("Vulkan instance is not initialized".to_string())?;

        let device = self
            .device
            .as_ref()
            .ok_or("Vulkan device is not initialized".to_string())?;

        let camera = self
            .camera
            .as_ref()
            .ok_or("Vulkan camera is not initialized".to_string())?;

        let surface = self
            .surface
            .as_ref()
            .ok_or("Vulkan surface is not initialized".to_string())?;

        let old_swapchain = self
            .swapchain
            .as_ref()
            .ok_or("Vulkan swapchain is not initialized".to_string())?;

        unsafe {
            device
                .raw()
                .device_wait_idle()
                .map_err(|error| format!("failed waiting for Vulkan device: {error:?}"))?;
        }

        engine_logger::info!("Recreating swapchain: {}x{}", size.width, size.height);

        let new_swapchain = VulkanSwapchain::recreate(
            instance.raw(),
            device,
            surface,
            size.width,
            size.height,
            old_swapchain,
        )
        .map_err(|error| format!("failed recreating swapchain: {error}"))?;

        let new_depth_buffer =
            VulkanDepthBuffer::new(instance.raw(), device, new_swapchain.extent())
                .map_err(|error| format!("failed recreating depth buffer: {error}"))?;

        let part = self
            .parts
            .first()
            .ok_or("no model parts are initialized".to_string())?;

        let new_pipelines = VulkanMaterialPipelines::new(
            device.raw(),
            new_swapchain.format(),
            new_depth_buffer.format(),
            camera.descriptor_set_layout(),
            part.material.descriptor_set_layout(),
        )
        .map_err(|error| format!("failed recreating graphics pipeline: {error}"))?;

        let new_frame = VulkanFrame::new(device, &new_swapchain)
            .map_err(|error| format!("failed recreating frame resources: {error}"))?;

        self.frame = Some(new_frame);
        self.pipelines = Some(new_pipelines);
        self.depth_buffer = Some(new_depth_buffer);
        self.swapchain = Some(new_swapchain);
        self.swapchain_dirty = false;

        engine_logger::info!("Swapchain recreated.");

        Ok(true)
    }
}

impl ApplicationHandler for GameApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title(self.project_name.as_deref().unwrap_or("Game Engine"))
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 720));

        let window = event_loop
            .create_window(attributes)
            .expect("failed to create game window");

        let display_handle = window
            .display_handle()
            .expect("failed to get display handle")
            .as_raw();

        let window_handle = window
            .window_handle()
            .expect("failed to get window handle")
            .as_raw();

        engine_logger::info!("Creating Vulkan instance...");

        let instance =
            VulkanInstance::for_display(display_handle).expect("failed to create Vulkan instance");

        engine_logger::info!("Creating Vulkan surface...");

        let surface = VulkanSurface::new(&instance, display_handle, window_handle)
            .expect("failed to create Vulkan surface");

        engine_logger::info!("Vulkan surface created.");
        engine_logger::info!("Selecting presentation-capable GPU...");

        let device = VulkanDevice::for_surface(instance.raw(), &surface)
            .expect("failed to create Vulkan device");

        engine_logger::info!("Creating camera resources...");

        let camera = VulkanCamera::new(instance.raw(), &device)
            .expect("failed to create Vulkan camera resources");

        engine_logger::info!("Camera resources created.");

        let queues = device.queue_families();

        engine_logger::info!("Graphics queue family: {}", queues.graphics);
        engine_logger::info!("Present queue family: {}", queues.present);

        let size = window.inner_size();

        engine_logger::info!("Creating swapchain {}x{}...", size.width, size.height);

        let swapchain = VulkanSwapchain::new(
            instance.raw(),
            &device,
            &surface,
            size.width.max(1),
            size.height.max(1),
        )
        .expect("failed to create Vulkan swapchain");

        engine_logger::info!("Swapchain created.");
        engine_logger::info!("Swapchain images: {}", swapchain.images().len());
        engine_logger::info!(
            "Swapchain extent: {}x{}",
            swapchain.extent().width,
            swapchain.extent().height
        );
        engine_logger::info!("Swapchain format: {:?}", swapchain.format());

        engine_logger::info!("Creating depth buffer...");

        let depth_buffer = VulkanDepthBuffer::new(instance.raw(), &device, swapchain.extent())
            .expect("failed to create Vulkan depth buffer");

        engine_logger::info!("Depth buffer created.");

        let asset_root = if self.project_name.is_some() {
            std::env::current_exe()
                .expect("failed to locate executable")
                .parent()
                .expect("executable has no parent directory")
                .join("assets")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
        };

        let model_path = asset_root
            .join("props/robot.glb")
            .canonicalize()
            .expect("failed to resolve robot model path");

        let mut asset_manager =
            AssetManager::new(asset_root).expect("failed to create asset manager");

        // The loader now returns geometry and material data together.
        let imported_parts = match assets::load_first_mesh(&model_path) {
            Ok(data) => data,
            Err(error) => {
                engine_logger::error!("Failed to import {}: {}", model_path.display(), error);

                event_loop.exit();
                return;
            }
        };

        let mut parts = Vec::with_capacity(imported_parts.len());
        for imported in imported_parts {
            let mesh_key = format!(
                "{}#mesh:{}#primitive:{}",
                model_path.display(),
                imported.mesh_index,
                imported.primitive_index,
            );
            let image_key = match imported.base_color_image_index {
                Some(index) => format!("{}#image:{}", model_path.display(), index),
                None => "builtin:white".to_string(),
            };

            let mesh = match asset_manager.load_mesh(&instance, &device, mesh_key, &imported.mesh) {
                Ok(mesh) => mesh,
                Err(error) => {
                    engine_logger::error!(
                        "Failed to upload primitive {}: {}",
                        imported.primitive_index,
                        error
                    );
                    event_loop.exit();
                    return;
                }
            };
            let material = match asset_manager.load_material(
                &instance,
                &device,
                &image_key,
                &imported.material,
            ) {
                Ok(material) => material,
                Err(error) => {
                    engine_logger::error!(
                        "Failed to create primitive {} material: {}",
                        imported.primitive_index,
                        error
                    );
                    event_loop.exit();
                    return;
                }
            };
            engine_logger::info!(
                "Uploaded primitive {}: {} indices, double-sided {}",
                imported.primitive_index,
                mesh.index_count(),
                material.double_sided(),
            );
            parts.push(ModelPart { mesh, material });
        }
        engine_logger::info!(
            "Uploaded {} model parts; GPU meshes: {}, GPU textures: {}",
            parts.len(),
            asset_manager.loaded_mesh_count(),
            asset_manager.loaded_texture_count(),
        );

        // load_first_mesh rejects empty meshes. Texture descriptor layouts
        // currently have the same bindings for every material.
        let first_part = parts.first().expect("imported mesh has no parts");
        let pipelines = VulkanMaterialPipelines::new(
            device.raw(),
            swapchain.format(),
            depth_buffer.format(),
            camera.descriptor_set_layout(),
            first_part.material.descriptor_set_layout(),
        )
        .expect("failed to create Vulkan graphics pipeline");

        engine_logger::info!("Graphics pipeline created.");
        engine_logger::info!("Creating frame resources...");

        let frame =
            VulkanFrame::new(&device, &swapchain).expect("failed to create Vulkan frame resources");

        engine_logger::info!("Frame resources created.");

        self.frame = Some(frame);
        self.parts = parts;
        self.pipelines = Some(pipelines);
        self.camera = Some(camera);
        self.asset_manager = Some(asset_manager);
        self.depth_buffer = Some(depth_buffer);
        self.swapchain = Some(swapchain);
        self.device = Some(device);
        self.surface = Some(surface);
        self.instance = Some(instance);
        self.window = Some(window);
        self.start_time = Some(Instant::now());

        engine_logger::component_health(
            "renderer",
            "HEALTHY",
            false,
            "Game rendering resources initialized",
        );

        self.window.as_ref().unwrap().request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = &self.window else {
            return;
        };

        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::RedrawRequested => {
                if self
                    .last_heartbeat
                    .is_none_or(|last| last.elapsed().as_secs() >= 5)
                {
                    engine_logger::heartbeat("game");
                    self.last_heartbeat = Some(Instant::now());
                }

                let size = match self.window.as_ref() {
                    Some(window) => window.inner_size(),
                    None => return,
                };

                if size.width == 0 || size.height == 0 {
                    return;
                }

                if self.swapchain_dirty {
                    match self.recreate_swapchain() {
                        Ok(true) => {}
                        Ok(false) => return,
                        Err(error) => {
                            engine_logger::error!("{error}");
                            event_loop.exit();
                            return;
                        }
                    }
                }

                let extent = self.swapchain.as_ref().unwrap().extent();
                let aspect_ratio = extent.width as f32 / extent.height as f32;

                let camera_uniform = CameraUniform::perspective(
                    [0.0, 0.0, 3.0],
                    [0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    60.0_f32.to_radians(),
                    aspect_ratio,
                    0.1,
                    100.0,
                );

                let elapsed = self
                    .start_time
                    .as_ref()
                    .map(|start| start.elapsed().as_secs_f32())
                    .unwrap_or(0.0);

                if self.parts.is_empty() {
                    return;
                }

                let result = match (
                    self.frame.as_mut(),
                    self.device.as_ref(),
                    self.swapchain.as_ref(),
                    self.depth_buffer.as_ref(),
                    self.pipelines.as_ref(),
                    self.camera.as_ref(),
                ) {
                    (
                        Some(frame),
                        Some(device),
                        Some(swapchain),
                        Some(depth_buffer),
                        Some(pipelines),
                        Some(camera),
                    ) => {
                        let transforms = [
                            ModelPushConstants::from_3d(
                                [-0.8, 0.0, 0.0],
                                if self.project_name.is_some() {
                                    [0.0; 3]
                                } else {
                                    [elapsed * 0.6, elapsed * 0.9, 0.0]
                                },
                                [0.7, 0.7, 0.7],
                            ),
                            ModelPushConstants::from_3d(
                                [0.8, 0.0, 0.0],
                                if self.project_name.is_some() {
                                    [0.0; 3]
                                } else {
                                    [elapsed * -0.5, elapsed * 0.75, elapsed * 0.2]
                                },
                                [0.7, 0.7, 0.7],
                            ),
                        ];

                        let mut objects = Vec::with_capacity(self.parts.len() * transforms.len());
                        for transform in transforms {
                            for part in &self.parts {
                                objects.push(RenderObject::new(
                                    part.mesh.as_ref(),
                                    &part.material,
                                    transform,
                                ));
                            }
                        }

                        frame.draw(
                            device,
                            swapchain,
                            depth_buffer,
                            pipelines,
                            camera,
                            &camera_uniform,
                            &objects,
                        )
                    }

                    _ => return,
                };

                match result.log_error_for("game::renderer", "render frame") {
                    Ok(FrameStatus::Rendered) => {}

                    Ok(FrameStatus::SwapchainNeedsRebuild) => {
                        self.swapchain_dirty = true;
                    }

                    Err(_) => {
                        event_loop.exit();
                        return;
                    }
                }

                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            WindowEvent::Resized(size) => {
                self.swapchain_dirty = true;

                if size.width == 0 || size.height == 0 {
                    return;
                }

                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }

            _ => {}
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

impl Drop for GameApp {
    fn drop(&mut self) {
        if let Some(device) = self.device.as_ref() {
            unsafe {
                let _ = device.raw().device_wait_idle();
            }
        }
    }
}
