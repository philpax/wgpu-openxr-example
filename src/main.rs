use anyhow::Context;
use std::borrow::Cow;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

mod xr;

pub struct WgpuState {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn main() -> anyhow::Result<()> {
    let wgpu_features = wgpu::Features::MULTIVIEW;
    let wgpu_limits = wgpu::Limits::default();

    let event_loop = EventLoop::new();
    let window = winit::window::Window::new(&event_loop)?;

    let xr_enabled = std::env::args().any(|a| a == "--xr");
    let (wgpu_state, surface, mut xr_state) = if xr_enabled {
        let (wgpu_state, xr_state) = xr::XrState::initialize_with_wgpu(wgpu_features, wgpu_limits)?;
        let surface = unsafe { wgpu_state.instance.create_surface(&window) };
        (wgpu_state, surface, Some(xr_state))
    } else {
        let instance = wgpu::Instance::new(wgpu::Backends::all());
        let surface = unsafe { instance.create_surface(&window) };
        let adapter =
            futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                force_fallback_adapter: false,
                // Request an adapter which can render to our surface
                compatible_surface: Some(&surface),
            }))
            .context("Failed to find an appropriate adapter")?;

        // Create the logical device and command queue
        let (device, queue) = futures::executor::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu_features,
                limits: wgpu_limits,
            },
            None,
        ))
        .context("Failed to create device")?;
        (
            WgpuState {
                instance,
                adapter,
                device,
                queue,
            },
            surface,
            None,
        )
    };

    let size = window.inner_size();

    // Load the shaders from disk
    let shader = wgpu_state
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });

    let pipeline_layout =
        wgpu_state
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[],
                push_constant_ranges: &[],
            });

    let swapchain_format = surface.get_supported_formats(&wgpu_state.adapter)[0];

    let render_pipeline =
        wgpu_state
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(swapchain_format.into())],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

    let mut config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: swapchain_format,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Fifo,
    };

    surface.configure(&wgpu_state.device, &config);

    event_loop.run(move |event, _, control_flow| {
        // Have the closure take ownership of the resources.
        // `event_loop.run` never returns, therefore we must do this to ensure
        // the resources are properly cleaned up.
        let _ = (&xr_state, &wgpu_state, &shader, &pipeline_layout);

        let xr_frame_state = xr_state.as_mut().and_then(|x| x.pre_frame().unwrap());

        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                // Reconfigure the surface with the new size
                config.width = size.width;
                config.height = size.height;
                surface.configure(&wgpu_state.device, &config);
                // On macos the window needs to be redrawn manually after resizing
                window.request_redraw();
            }
            Event::RedrawRequested(_) => {
                let frame = surface
                    .get_current_texture()
                    .expect("Failed to acquire next swap chain texture");
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let mut encoder = wgpu_state
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::GREEN),
                                store: true,
                            },
                        })],
                        depth_stencil_attachment: None,
                    });
                    rpass.set_pipeline(&render_pipeline);
                    rpass.draw(0..3, 0..1);
                }

                wgpu_state.queue.submit(Some(encoder.finish()));
                frame.present();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }

        if let Some((xr_state, xr_frame_state)) = xr_state.as_mut().zip(xr_frame_state) {
            xr_state.post_frame(xr_frame_state).unwrap();
        }
    });
}
