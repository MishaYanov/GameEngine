use std::time::Instant;

use renderer::vulkan::{
    CameraUniform, FrameStatus, ModelPushConstants, RenderObject, Vertex, VulkanCamera,
    VulkanDepthBuffer, VulkanDevice, VulkanFrame, VulkanGraphicsPipeline, VulkanInstance,
    VulkanMaterial, VulkanMesh, VulkanSurface, VulkanSwapchain, VulkanTexture,
};

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::{Window, WindowId},
};

#[derive(Default)]
struct GameApp {
    frame: Option<VulkanFrame>,
    mesh: Option<VulkanMesh>,
    pipeline: Option<VulkanGraphicsPipeline>,
    camera: Option<VulkanCamera>,
    materials: Vec<VulkanMaterial>,
    depth_buffer: Option<VulkanDepthBuffer>,
    swapchain: Option<VulkanSwapchain>,
    device: Option<VulkanDevice>,
    surface: Option<VulkanSurface>,
    instance: Option<VulkanInstance>,
    window: Option<Window>,
    start_time: Option<Instant>,
    swapchain_dirty: bool,
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

        println!("Recreating swapchain: {}x{}", size.width, size.height,);

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

        let material = self
            .materials
            .first()
            .ok_or("no Vulkan materials are initialized".to_string())?;

        let new_pipeline = VulkanGraphicsPipeline::new(
            device.raw(),
            new_swapchain.format(),
            new_depth_buffer.format(),
            camera.descriptor_set_layout(),
            material.descriptor_set_layout(),
        )
        .map_err(|error| format!("failed recreating graphics pipeline: {error}"))?;

        let new_frame = VulkanFrame::new(device, &new_swapchain)
            .map_err(|error| format!("failed recreating frame resources: {error}"))?;

        self.frame = Some(new_frame);

        self.pipeline = Some(new_pipeline);

        self.depth_buffer = Some(new_depth_buffer);

        self.swapchain = Some(new_swapchain);

        self.swapchain_dirty = false;

        println!("Swapchain recreated.");

        Ok(true)
    }
}

impl ApplicationHandler for GameApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attributes = Window::default_attributes()
            .with_title("Game Engine")
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

        println!("Creating Vulkan instance...");

        let instance =
            VulkanInstance::for_display(display_handle).expect("failed to create Vulkan instance");

        println!("Creating Vulkan surface...");

        let surface = VulkanSurface::new(&instance, display_handle, window_handle)
            .expect("failed to create Vulkan surface");

        println!("Vulkan surface created.");

        println!("Selecting presentation-capable GPU...");

        let device = VulkanDevice::for_surface(instance.raw(), &surface)
            .expect("failed to create Vulkan device");

        println!("Creating camera resources...");

        let camera = VulkanCamera::new(instance.raw(), &device)
            .expect("failed to create Vulkan camera resources");

        println!("Camera resources created.");

        let queues = device.queue_families();

        println!("Graphics queue family: {}", queues.graphics,);

        println!("Present queue family: {}", queues.present,);

        let size = window.inner_size();

        println!("Creating swapchain {}x{}...", size.width, size.height,);

        let swapchain = VulkanSwapchain::new(
            instance.raw(),
            &device,
            &surface,
            size.width.max(1),
            size.height.max(1),
        )
        .expect("failed to create Vulkan swapchain");

        println!("Swapchain created.");

        println!("Swapchain images: {}", swapchain.images().len(),);

        println!(
            "Swapchain extent: {}x{}",
            swapchain.extent().width,
            swapchain.extent().height,
        );

        println!("Swapchain format: {:?}", swapchain.format(),);

        println!("Creating depth buffer...");

        let depth_buffer = VulkanDepthBuffer::new(instance.raw(), &device, swapchain.extent())
            .expect("failed to create Vulkan depth buffer");

        println!("Depth buffer created.");

        let vertices = [
            /*
             * Front +Z
             */
            Vertex::new([-0.5, -0.5, 0.5], [0.0, 0.0]),
            Vertex::new([0.5, -0.5, 0.5], [1.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.5], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, 0.5], [0.0, 1.0]),
            /*
             * Back -Z
             */
            Vertex::new([0.5, -0.5, -0.5], [0.0, 0.0]),
            Vertex::new([-0.5, -0.5, -0.5], [1.0, 0.0]),
            Vertex::new([-0.5, 0.5, -0.5], [1.0, 1.0]),
            Vertex::new([0.5, 0.5, -0.5], [0.0, 1.0]),
            /*
             * Left -X
             */
            Vertex::new([-0.5, -0.5, -0.5], [0.0, 0.0]),
            Vertex::new([-0.5, -0.5, 0.5], [1.0, 0.0]),
            Vertex::new([-0.5, 0.5, 0.5], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, -0.5], [0.0, 1.0]),
            /*
             * Right +X
             */
            Vertex::new([0.5, -0.5, 0.5], [0.0, 0.0]),
            Vertex::new([0.5, -0.5, -0.5], [1.0, 0.0]),
            Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0]),
            Vertex::new([0.5, 0.5, 0.5], [0.0, 1.0]),
            /*
             * Top +Y
             */
            Vertex::new([-0.5, 0.5, 0.5], [0.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.5], [1.0, 0.0]),
            Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, -0.5], [0.0, 1.0]),
            /*
             * Bottom -Y
             */
            Vertex::new([-0.5, -0.5, -0.5], [0.0, 0.0]),
            Vertex::new([0.5, -0.5, -0.5], [1.0, 0.0]),
            Vertex::new([0.5, -0.5, 0.5], [1.0, 1.0]),
            Vertex::new([-0.5, -0.5, 0.5], [0.0, 1.0]),
        ];

        let indices: [u16; 36] = [
            0, 1, 2, 2, 3, 0, 4, 5, 6, 6, 7, 4, 8, 9, 10, 10, 11, 8, 12, 13, 14, 14, 15, 12, 16,
            17, 18, 18, 19, 16, 20, 21, 22, 22, 23, 20,
        ];

        println!("Creating mesh...");

        let mesh = VulkanMesh::new(instance.raw(), &device, &vertices, &indices)
            .expect("failed to create Vulkan mesh");

        println!("Mesh created.");

        println!("Creating materials...");

        /*
         * Material A:
         * neutral black / white checkerboard.
         */
        let texture_a = VulkanTexture::checkerboard(instance.raw(), &device)
            .expect("failed to create texture A");

        let material_a = VulkanMaterial::new(texture_a);

        /*
         * Material B:
         * blue / orange checkerboard.
         */
        let texture_b = VulkanTexture::checkerboard_with_colors(
            instance.raw(),
            &device,
            [30, 110, 230, 255],
            [240, 120, 30, 255],
        )
        .expect("failed to create texture B");

        let material_b = VulkanMaterial::new(texture_b);

        println!("Materials created.");

        println!("Creating graphics pipeline...");

        let pipeline = VulkanGraphicsPipeline::new(
            device.raw(),
            swapchain.format(),
            depth_buffer.format(),
            camera.descriptor_set_layout(),
            material_a.descriptor_set_layout(),
        )
        .expect("failed to create Vulkan graphics pipeline");

        println!("Graphics pipeline created.");

        println!("Creating frame resources...");

        let frame =
            VulkanFrame::new(&device, &swapchain).expect("failed to create Vulkan frame resources");

        println!("Frame resources created.");

        self.frame = Some(frame);

        self.mesh = Some(mesh);

        self.pipeline = Some(pipeline);

        self.camera = Some(camera);

        self.materials = vec![material_a, material_b];

        self.depth_buffer = Some(depth_buffer);

        self.swapchain = Some(swapchain);

        self.device = Some(device);

        self.surface = Some(surface);

        self.instance = Some(instance);

        self.window = Some(window);

        self.start_time = Some(Instant::now());

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
                let size = match self.window.as_ref() {
                    Some(window) => window.inner_size(),

                    None => {
                        return;
                    }
                };

                if size.width == 0 || size.height == 0 {
                    return;
                }

                if self.swapchain_dirty {
                    match self.recreate_swapchain() {
                        Ok(true) => {}

                        Ok(false) => {
                            return;
                        }

                        Err(error) => {
                            eprintln!("{error}");

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

                let Some(material_a) = self.materials.get(0) else {
                    return;
                };

                let Some(material_b) = self.materials.get(1) else {
                    return;
                };

                let result = match (
                    self.frame.as_mut(),
                    self.device.as_ref(),
                    self.swapchain.as_ref(),
                    self.depth_buffer.as_ref(),
                    self.pipeline.as_ref(),
                    self.camera.as_ref(),
                    self.mesh.as_ref(),
                ) {
                    (
                        Some(frame),
                        Some(device),
                        Some(swapchain),
                        Some(depth_buffer),
                        Some(pipeline),
                        Some(camera),
                        Some(mesh),
                    ) => {
                        let objects = [

                            RenderObject::new(
                                mesh,
                                material_a,
                                ModelPushConstants::from_3d(
                                    [-0.8, 0.0, 0.0],
                                    [elapsed * 0.6, elapsed * 0.9, 0.0],
                                    [0.7, 0.7, 0.7],
                                ),
                            ),

                            RenderObject::new(
                                mesh,
                                material_b,
                                ModelPushConstants::from_3d(
                                    [0.8, 0.0, 0.0],
                                    [elapsed * -0.5, elapsed * 0.75, elapsed * 0.2],
                                    [0.7, 0.7, 0.7],
                                ),
                            ),
                        ];

                        frame.draw(
                            device,
                            swapchain,
                            depth_buffer,
                            pipeline,
                            &camera,
                            &camera_uniform,
                            &objects,
                        )
                    }

                    _ => {
                        return;
                    }
                };

                match result {
                    Ok(FrameStatus::Rendered) => {}

                    Ok(FrameStatus::SwapchainNeedsRebuild) => {
                        self.swapchain_dirty = true;
                    }

                    Err(error) => {
                        eprintln!("Rendering failed: {error}");

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
    let event_loop = EventLoop::new().expect("failed to create event loop");

    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = GameApp::default();

    event_loop
        .run_app(&mut app)
        .expect("game event loop failed");
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
