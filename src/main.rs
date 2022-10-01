use anyhow::Context;
use glam::{vec3, vec4, Mat4, Vec3, Vec4};
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

struct PerspectiveCamera {
    eye: Vec3,
    target: Vec3,
    up: Vec3,

    aspect_ratio: f32,
    fov_y_rad: f32,
    z_near: f32,
    z_far: f32,
}
impl PerspectiveCamera {
    fn to_view_proj_matrix(&self) -> Mat4 {
        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_rh(self.fov_y_rad, self.aspect_ratio, self.z_near, self.z_far);

        proj * view
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

    let mut camera = PerspectiveCamera {
        eye: Vec3::ZERO,
        target: vec3(0.0, 0.0, 1.0),
        up: Vec3::Y,
        aspect_ratio: {
            let winit::dpi::PhysicalSize { width, height } = window.inner_size().cast::<f32>();
            width / height
        },
        fov_y_rad: 90.0f32.to_radians(),
        z_near: 0.05,
        z_far: 1000.0,
    };
    let camera_buffer = wgpu_state
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera buffer"),
            contents: bytemuck::cast_slice(&camera.to_view_proj_matrix().to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
    let camera_bind_group_layout =
        wgpu_state
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("camera_bind_group_layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });
    let camera_bind_group = wgpu_state
        .device
        .create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

    let swapchain_format = surface.get_supported_formats(&wgpu_state.adapter)[0];
    let pipeline_layout =
        wgpu_state
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&camera_bind_group_layout],
                push_constant_ranges: &[],
            });
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
        present_mode: wgpu::PresentMode::Immediate,
    };

    surface.configure(&wgpu_state.device, &config);

    let start_time = std::time::Instant::now();
    let (mut fps_timer, mut fps_count) = (std::time::Instant::now(), 0);
    event_loop.run(move |event, _, control_flow| {
        // Have the closure take ownership of the resources.
        // `event_loop.run` never returns, therefore we must do this to ensure
        // the resources are properly cleaned up.
        #[cfg(feature = "xr")]
        let _ = &xr_state;
        let _ = (&wgpu_state, &shader, &pipeline_layout);

        let mut cleared = false;

        *control_flow = ControlFlow::Poll;
        match event {
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                // Reconfigure the surface with the new size
                config.width = size.width;
                config.height = size.height;
                surface.configure(&wgpu_state.device, &config);

                camera.aspect_ratio = (size.width as f32) / (size.height as f32);

                // On macos the window needs to be redrawn manually after resizing
                window.request_redraw();
            }
            Event::MainEventsCleared => {
                window.request_redraw();
                cleared = true;
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }

        if !cleared {
            return;
        }

        #[cfg(feature = "xr")]
        let xr_frame_state = xr_state.as_mut().and_then(|x| x.pre_frame().unwrap());

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
            rpass.set_bind_group(0, &camera_bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }

        camera.eye.z = start_time.elapsed().as_secs_f32().cos() - 1.0;
        wgpu_state.queue.write_buffer(
            &camera_buffer,
            0,
            bytemuck::cast_slice(&camera.to_view_proj_matrix().to_cols_array()),
        );
        wgpu_state.queue.submit(Some(encoder.finish()));
        frame.present();

        #[cfg(feature = "xr")]
        if let Some((xr_state, xr_frame_state)) = xr_state.as_mut().zip(xr_frame_state) {
            xr_state.post_frame(xr_frame_state).unwrap();
        }

        fps_count += 1;
        if fps_timer.elapsed().as_millis() > 1_000 {
            window.set_title(&format!(
                "wgpu-openxr-example: {:.02} FPS",
                (fps_count as f32) / fps_timer.elapsed().as_secs_f32()
            ));

            fps_count = 0;
            fps_timer = std::time::Instant::now();
        }
    });
}
