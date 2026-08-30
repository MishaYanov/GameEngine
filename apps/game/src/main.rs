use renderer::vulkan::{
    FrameStatus,
    ModelPushConstants,
    Vertex,
    VulkanDevice,
    VulkanFrame,
    VulkanGraphicsPipeline,
    VulkanInstance,
    VulkanMesh,
    VulkanSurface,
    VulkanSwapchain,
};

use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{
        ActiveEventLoop,
        ControlFlow,
        EventLoop,
    },
    raw_window_handle::{
        HasDisplayHandle,
        HasWindowHandle,
    },
    window::{
        Window,
        WindowId,
    },
};

#[derive(Default)]
struct GameApp {
    frame: Option<VulkanFrame>,
    mesh: Option<VulkanMesh>,
    pipeline: Option<VulkanGraphicsPipeline>,
    swapchain: Option<VulkanSwapchain>,
    device: Option<VulkanDevice>,
    surface: Option<VulkanSurface>,
    instance: Option<VulkanInstance>,
    window: Option<Window>,

    swapchain_dirty: bool,
}

impl GameApp {
    fn recreate_swapchain(
        &mut self,
    ) -> Result<bool, String> {
        let size = self
            .window
            .as_ref()
            .ok_or(
                "window is not initialized"
                    .to_string(),
            )?
            .inner_size();

        if size.width == 0
            || size.height == 0
        {
            return Ok(false);
        }

        let instance = self
            .instance
            .as_ref()
            .ok_or(
                "Vulkan instance is not initialized"
                    .to_string(),
            )?;

        let device = self
            .device
            .as_ref()
            .ok_or(
                "Vulkan device is not initialized"
                    .to_string(),
            )?;

        let surface = self
            .surface
            .as_ref()
            .ok_or(
                "Vulkan surface is not initialized"
                    .to_string(),
            )?;

        let old_swapchain = self
            .swapchain
            .as_ref()
            .ok_or(
                "Vulkan swapchain is not initialized"
                    .to_string(),
            )?;

        unsafe {
            device
                .raw()
                .device_wait_idle()
                .map_err(
                    |error| {
                        format!(
                            "failed waiting for Vulkan device: {error:?}"
                        )
                    },
                )?;
        }

        println!(
            "Recreating swapchain: {}x{}",
            size.width,
            size.height,
        );

        let new_swapchain =
            VulkanSwapchain::recreate(
                instance.raw(),
                device,
                surface,
                size.width,
                size.height,
                old_swapchain,
            )
                .map_err(
                    |error| {
                        format!(
                            "failed recreating swapchain: {error}"
                        )
                    },
                )?;

        let new_pipeline =
            VulkanGraphicsPipeline::new(
                device.raw(),
                new_swapchain.format(),
            )
                .map_err(
                    |error| {
                        format!(
                            "failed recreating graphics pipeline: {error}"
                        )
                    },
                )?;

        let new_frame =
            VulkanFrame::new(
                device,
                &new_swapchain,
            )
                .map_err(
                    |error| {
                        format!(
                            "failed recreating frame resources: {error}"
                        )
                    },
                )?;

        /*
		 * Replace resources in dependency order:
		 *
		 * frame -> pipeline -> swapchain
		 */
        self.frame =
            Some(new_frame);

        self.pipeline =
            Some(new_pipeline);

        self.swapchain =
            Some(new_swapchain);

        self.swapchain_dirty =
            false;

        println!(
            "Swapchain recreated."
        );

        Ok(true)
    }
}

impl ApplicationHandler for GameApp {
    fn resumed(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) {
        if self.window.is_some() {
            return;
        }

        let attributes =
            Window::default_attributes()
                .with_title(
                    "Game Engine",
                )
                .with_inner_size(
                    winit::dpi::
                    LogicalSize::new(
                        1280,
                        720,
                    ),
                );

        let window =
            event_loop
                .create_window(
                    attributes,
                )
                .expect(
                    "failed to create game window",
                );

        let display_handle =
            window
                .display_handle()
                .expect(
                    "failed to get display handle",
                )
                .as_raw();

        let window_handle =
            window
                .window_handle()
                .expect(
                    "failed to get window handle",
                )
                .as_raw();

        println!(
            "Creating Vulkan instance..."
        );

        let instance =
            VulkanInstance::for_display(
                display_handle,
            )
                .expect(
                    "failed to create Vulkan instance",
                );

        println!(
            "Creating Vulkan surface..."
        );

        let surface =
            VulkanSurface::new(
                &instance,
                display_handle,
                window_handle,
            )
                .expect(
                    "failed to create Vulkan surface",
                );

        println!(
            "Vulkan surface created."
        );

        println!(
            "Selecting presentation-capable GPU..."
        );

        let device =
            VulkanDevice::for_surface(
                instance.raw(),
                &surface,
            )
                .expect(
                    "failed to create Vulkan device",
                );

        let queues =
            device.queue_families();

        println!(
            "Graphics queue family: {}",
            queues.graphics,
        );

        println!(
            "Present queue family: {}",
            queues.present,
        );

        let size =
            window.inner_size();

        println!(
            "Creating swapchain {}x{}...",
            size.width,
            size.height,
        );

        let swapchain =
            VulkanSwapchain::new(
                instance.raw(),
                &device,
                &surface,
                size.width.max(1),
                size.height.max(1),
            )
                .expect(
                    "failed to create Vulkan swapchain",
                );

        println!(
            "Swapchain created."
        );

        println!(
            "Swapchain images: {}",
            swapchain
                .images()
                .len(),
        );

        println!(
            "Swapchain extent: {}x{}",
            swapchain
                .extent()
                .width,

            swapchain
                .extent()
                .height,
        );

        println!(
            "Swapchain format: {:?}",
            swapchain.format(),
        );

        /*
		 * Static mesh geometry.
		 *
		 * VulkanMesh uploads these through staging
		 * buffers into DEVICE_LOCAL GPU memory.
		 */
        let vertices = [
            Vertex::new(
                [
                    0.0,
                    -0.5,
                ],
                [
                    1.0,
                    0.0,
                    0.0,
                ],
            ),

            Vertex::new(
                [
                    0.5,
                    0.5,
                ],
                [
                    0.0,
                    1.0,
                    0.0,
                ],
            ),

            Vertex::new(
                [
                    -0.5,
                    0.5,
                ],
                [
                    0.0,
                    0.0,
                    1.0,
                ],
            ),
        ];

        let indices: [u16; 3] = [
            0,
            1,
            2,
        ];

        println!(
            "Creating mesh..."
        );

        let mesh =
            VulkanMesh::new(
                instance.raw(),
                &device,
                &vertices,
                &indices,
            )
                .expect(
                    "failed to create Vulkan mesh",
                );

        println!(
            "Mesh created."
        );

        println!(
            "Creating graphics pipeline..."
        );

        let pipeline =
            VulkanGraphicsPipeline::new(
                device.raw(),
                swapchain.format(),
            )
                .expect(
                    "failed to create Vulkan graphics pipeline",
                );

        println!(
            "Graphics pipeline created."
        );

        println!(
            "Creating frame resources..."
        );

        let frame =
            VulkanFrame::new(
                &device,
                &swapchain,
            )
                .expect(
                    "failed to create Vulkan frame resources",
                );

        println!(
            "Frame resources created."
        );

        self.frame =
            Some(frame);

        self.mesh =
            Some(mesh);

        self.pipeline =
            Some(pipeline);

        self.swapchain =
            Some(swapchain);

        self.device =
            Some(device);

        self.surface =
            Some(surface);

        self.instance =
            Some(instance);

        self.window =
            Some(window);

        self.window
            .as_ref()
            .unwrap()
            .request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) =
            &self.window
        else {
            return;
        };

        if window.id()
            != window_id
        {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                self.swapchain_dirty =
                    true;

                if size.width == 0
                    || size.height == 0
                {
                    return;
                }

                if let Some(window) =
                    self.window.as_ref()
                {
                    window
                        .request_redraw();
                }
            }

            WindowEvent::RedrawRequested => {
                let size =
                    match self
                        .window
                        .as_ref()
                    {
                        Some(window) => {
                            window.inner_size()
                        }

                        None => {
                            return;
                        }
                    };

                if size.width == 0
                    || size.height == 0
                {
                    return;
                }

                if self.swapchain_dirty {
                    match self
                        .recreate_swapchain()
                    {
                        Ok(true) => {}

                        Ok(false) => {
                            return;
                        }

                        Err(error) => {
                            eprintln!(
                                "{error}"
                            );

                            event_loop
                                .exit();

                            return;
                        }
                    }
                }

                /*
				 * Per-draw model transform.
				 *
				 * Mesh data remains unchanged.
				 *
				 * This matrix:
				 * - moves right
				 * - rotates 20 degrees
				 * - scales to 75%
				 */

                let models = [
                    ModelPushConstants::from_2d(
                        [
                            -0.55,
                            0.0,
                        ],

                        ModelPushConstants::
                        degrees_to_radians(
                            -20.0,
                        ),

                        [
                            0.45,
                            0.45,
                        ],
                    ),

                    ModelPushConstants::from_2d(
                        [
                            0.0,
                            0.0,
                        ],

                        0.0,

                        [
                            0.55,
                            0.55,
                        ],
                    ),

                    ModelPushConstants::from_2d(
                        [
                            0.55,
                            0.0,
                        ],

                        ModelPushConstants::
                        degrees_to_radians(
                            20.0,
                        ),

                        [
                            0.45,
                            0.45,
                        ],
                    ),
                ];

                let result =
                    match (
                        self.frame
                            .as_mut(),

                        self.device
                            .as_ref(),

                        self.swapchain
                            .as_ref(),

                        self.pipeline
                            .as_ref(),

                        self.mesh
                            .as_ref(),
                    ) {
                        (
                            Some(frame),
                            Some(device),
                            Some(swapchain),
                            Some(pipeline),
                            Some(mesh),
                        ) => {
                            frame.draw(
                                device,
                                swapchain,
                                pipeline,
                                mesh,
                                &models,
                            )
                        }

                        _ => {
                            return;
                        }
                    };

                match result {
                    Ok(
                        FrameStatus::
                        Rendered,
                    ) => {}

                    Ok(
                        FrameStatus::
                        SwapchainNeedsRebuild,
                    ) => {
                        self.swapchain_dirty =
                            true;
                    }

                    Err(error) => {
                        eprintln!(
                            "Rendering failed: {error}"
                        );

                        event_loop
                            .exit();

                        return;
                    }
                }

                if let Some(window) =
                    self.window.as_ref()
                {
                    window
                        .request_redraw();
                }
            }

            _ => {}
        }
    }
}

fn main() {
    let event_loop =
        EventLoop::new()
            .expect(
                "failed to create event loop",
            );

    event_loop.set_control_flow(
        ControlFlow::Poll,
    );

    let mut app =
        GameApp::default();

    event_loop
        .run_app(
            &mut app,
        )
        .expect(
            "game event loop failed",
        );
}

impl Drop for GameApp {
    fn drop(&mut self) {
        if let Some(device) =
            self.device.as_ref()
        {
            unsafe {
                let _ =
                    device
                        .raw()
                        .device_wait_idle();
            }
        }
    }
}