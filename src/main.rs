use anyhow::Context;
use glam::{vec3, vec4, Vec3, Vec4};
use std::borrow::Cow;
use wgpu::util::DeviceExt;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

#[cfg(feature = "xr")]
mod xr;

#[allow(dead_code)]
pub struct WgpuState {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn create_wgpu_state(
    window: &winit::window::Window,
    wgpu_features: wgpu::Features,
    wgpu_limits: wgpu::Limits,
) -> anyhow::Result<(WgpuState, wgpu::Surface)> {
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

    Ok((
        WgpuState {
            instance,
            adapter,
            device,
            queue,
        },
        surface,
    ))
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}
impl Vertex {
    fn new(position: Vec3, color: Vec4) -> Self {
        Self {
            position: position.to_array(),
            color: color.to_array(),
        }
    }
}

fn main() -> anyhow::Result<()> {
    let wgpu_features = wgpu::Features::MULTIVIEW;
    let wgpu_limits = wgpu::Limits::default();

    let event_loop = EventLoop::new();
    let window = winit::window::Window::new(&event_loop)?;

    #[cfg(feature = "xr")]
    let (wgpu_state, surface, mut xr_state) = if std::env::args().any(|a| a == "--xr") {
        let (wgpu_state, xr_state) = xr::XrState::initialize_with_wgpu(wgpu_features, wgpu_limits)?;
        let surface = unsafe { wgpu_state.instance.create_surface(&window) };
        (wgpu_state, surface, Some(xr_state))
    } else {
        let (wgpu_state, surface) = create_wgpu_state(&window, wgpu_features, wgpu_limits)?;
        (wgpu_state, surface, None)
    };

    #[cfg(not(feature = "xr"))]
    let (wgpu_state, surface) = create_wgpu_state(&window, wgpu_features, wgpu_limits)?;

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

    let triangle_vertex_buffer =
        wgpu_state
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&[
                    Vertex::new(vec3(-1.0, -1.0, 1.0), vec4(1.0, 0.0, 0.0, 1.0)),
                    Vertex::new(vec3(0.0, 1.0, 1.0), vec4(0.0, 1.0, 0.0, 1.0)),
                    Vertex::new(vec3(1.0, -1.0, 1.0), vec4(0.0, 0.0, 1.0, 1.0)),
                ]),
                usage: wgpu::BufferUsages::VERTEX,
            });

    let vertex_buffer_layout = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as _,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            },
            wgpu::VertexAttribute {
                offset: std::mem::size_of::<[f32; 3]>() as _,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    };

    let render_pipeline =
        wgpu_state
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[vertex_buffer_layout],
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
        #[cfg(feature = "xr")]
        let _ = &xr_state;
        let _ = (&wgpu_state, &shader, &pipeline_layout);

        #[cfg(feature = "xr")]
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
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: true,
                            },
                        })],
                        depth_stencil_attachment: None,
                    });
                    rpass.set_pipeline(&render_pipeline);
                    rpass.set_vertex_buffer(0, triangle_vertex_buffer.slice(..));
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

        #[cfg(feature = "xr")]
        if let Some((xr_state, xr_frame_state)) = xr_state.as_mut().zip(xr_frame_state) {
            xr_state.post_frame(xr_frame_state).unwrap();
        }
    });
}
