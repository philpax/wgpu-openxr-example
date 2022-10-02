use anyhow::Context;
use glam::{vec3, vec4, Mat4, Quat, Vec3, Vec4};
use std::{borrow::Cow, num::NonZeroU32};
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

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const VIEW_COUNT: u32 = 2;

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

    let mut camera_state = CameraState::new(&wgpu_state.device, window.inner_size());

    let swapchain_format = surface.get_supported_formats(&wgpu_state.adapter)[0];
    let mut main_state = MainState::new(&wgpu_state.device, &camera_state, swapchain_format);

    let mut config = {
        let size = window.inner_size();
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: swapchain_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Immediate,
        }
    };
    surface.configure(&wgpu_state.device, &config);
    let mut depth_texture = Texture::new_depth_texture(&wgpu_state.device, &config);
    let mut rt_texture = Texture::new_rt_texture(&wgpu_state.device, &config, swapchain_format);
    let mut blit_state = BlitState::new(&wgpu_state.device, &rt_texture.view, swapchain_format);

    let triangle_vertex_buffer =
        wgpu_state
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&[
                    Vertex::new(vec3(-1.0, -1.0, 0.0), vec4(1.0, 0.0, 0.0, 1.0)),
                    Vertex::new(vec3(0.0, 1.0, 0.0), vec4(0.0, 1.0, 0.0, 1.0)),
                    Vertex::new(vec3(1.0, -1.0, 0.0), vec4(0.0, 0.0, 1.0, 1.0)),
                ]),
                usage: wgpu::BufferUsages::VERTEX,
            });

    let start_time = std::time::Instant::now();
    let (mut fps_timer, mut fps_count) = (std::time::Instant::now(), 0);
    event_loop.run(move |event, _, control_flow| {
        // Have the closure take ownership of the resources.
        // `event_loop.run` never returns, therefore we must do this to ensure
        // the resources are properly cleaned up.
        #[cfg(feature = "xr")]
        let _ = &xr_state;
        let _ = (
            &wgpu_state,
            &triangle_vertex_buffer,
            &main_state,
            &depth_texture,
            &rt_texture,
            &blit_state,
        );

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
                depth_texture = Texture::new_depth_texture(&wgpu_state.device, &config);
                rt_texture = Texture::new_rt_texture(&wgpu_state.device, &config, swapchain_format);

                blit_state.resize(&wgpu_state.device, &rt_texture.view);
                camera_state.resize(size);

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

        let mut encoder = wgpu_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        main_state.encode_draw_pass(
            &mut encoder,
            &rt_texture.view,
            &depth_texture.view,
            &triangle_vertex_buffer,
            &camera_state.bind_group,
        );

        let frame = surface
            .get_current_texture()
            .expect("Failed to acquire next swap chain texture");
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        blit_state.encode_draw_pass(&mut encoder, &view);

        let time_since_start = start_time.elapsed().as_secs_f32();
        camera_state.data.eye.z = time_since_start.cos() - 1.0;
        wgpu_state.queue.write_buffer(
            &camera_state.buffer,
            0,
            bytemuck::cast_slice(&camera_state.data.to_view_proj_matrices()),
        );

        main_state.instances[0].1 = Quat::from_rotation_y(time_since_start / std::f32::consts::PI);
        main_state.upload_instances(&wgpu_state.queue);

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

struct MainState {
    #[allow(dead_code)]
    shader: wgpu::ShaderModule,
    #[allow(dead_code)]
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    instances: [(Vec3, Quat); 3],
    instance_buffer: wgpu::Buffer,
}
impl MainState {
    fn new(
        device: &wgpu::Device,
        camera_state: &CameraState,
        swapchain_format: wgpu::TextureFormat,
    ) -> Self {
        let instances = [
            (vec3(0.0, 0.0, 1.0), Quat::IDENTITY),
            (vec3(1.0, 0.0, 2.0), Quat::IDENTITY),
            (vec3(-1.0, 0.0, 2.0), Quat::IDENTITY),
        ];
        let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&Self::instances_to_data(&instances)),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let instance_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: (std::mem::size_of::<f32>() * 4 * 4) as _,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &(0..4)
                .into_iter()
                .map(|i| wgpu::VertexAttribute {
                    offset: (i * std::mem::size_of::<f32>() * 4) as _,
                    shader_location: 2 + i as u32,
                    format: wgpu::VertexFormat::Float32x4,
                })
                .collect::<Vec<_>>(),
        };

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("main.wgsl"))),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&camera_state.bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[vertex_buffer_layout, instance_buffer_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: NonZeroU32::new(VIEW_COUNT),
        });
        Self {
            shader,
            pipeline_layout,
            pipeline,

            instances,
            instance_buffer,
        }
    }
    fn upload_instances(&self, queue: &wgpu::Queue) {
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&Self::instances_to_data(&self.instances)),
        );
    }
    fn instances_to_data(poses: &[(Vec3, Quat)]) -> Vec<f32> {
        poses
            .into_iter()
            .flat_map(|(t, r)| {
                Mat4::from(glam::Affine3A::from_scale_rotation_translation(
                    Vec3::ONE,
                    *r,
                    *t,
                ))
                .to_cols_array()
            })
            .collect()
    }
    fn encode_draw_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        rt_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        vertex_buffer: &wgpu::Buffer,
        camera_bind_group: &wgpu::BindGroup,
    ) {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &rt_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: true,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: true,
                }),
                stencil_ops: None,
            }),
        });
        rpass.set_pipeline(&self.pipeline);
        rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
        rpass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        rpass.set_bind_group(0, &camera_bind_group, &[]);
        rpass.draw(0..3, 0..(self.instances.len() as u32));
    }
}

struct CameraState {
    data: PerspectiveCamera,

    buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    #[allow(dead_code)]
    bind_group: wgpu::BindGroup,
}
impl CameraState {
    fn new(device: &wgpu::Device, inner_size: winit::dpi::PhysicalSize<u32>) -> Self {
        let data = PerspectiveCamera {
            eye: Vec3::ZERO,
            target: vec3(0.0, 0.0, 1.0),
            up: Vec3::Y,
            aspect_ratio: inner_size.width as f32 / inner_size.height as f32,
            fov_y_rad: 90.0f32.to_radians(),
            z_near: 0.05,
            z_far: 1000.0,
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera buffer"),
            contents: bytemuck::cast_slice(&data.to_view_proj_matrices()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Camera Bind Group Layout"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self {
            data,
            buffer,
            bind_group_layout,
            bind_group,
        }
    }
    fn resize(&mut self, inner_size: winit::dpi::PhysicalSize<u32>) {
        self.data.aspect_ratio = inner_size.width as f32 / inner_size.height as f32;
    }
}

struct BlitState {
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    render_pipeline: wgpu::RenderPipeline,
    #[allow(dead_code)]
    vertex_buffer: wgpu::Buffer,
}
impl BlitState {
    fn new(
        device: &wgpu::Device,
        render_target_view: &wgpu::TextureView,
        swapchain_format: wgpu::TextureFormat,
    ) -> Self {
        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct BlitVertex {
            position: [f32; 3],
            uv_coords: [f32; 2],
        }
        impl BlitVertex {
            fn new(position: Vec3, uv_coords: [f32; 2]) -> Self {
                Self {
                    position: position.to_array(),
                    uv_coords,
                }
            }
        }

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("bind_group_layout"),
        });
        let bind_group =
            Self::create_bind_group(device, &bind_group_layout, render_target_view, &sampler);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let vertex_buffer_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BlitVertex>() as _,
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
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        };
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("blit.wgsl"))),
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "blit_vs_main",
                buffers: &[vertex_buffer_layout],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "blit_fs_main",
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Blit Vertex Buffer"),
            contents: bytemuck::cast_slice(&[
                BlitVertex::new(vec3(1.0, 1.0, 0.0), [1.0, 0.0]),
                BlitVertex::new(vec3(-1.0, 1.0, 0.0), [0.0, 0.0]),
                BlitVertex::new(vec3(-1.0, -1.0, 0.0), [0.0, 1.0]),
                //
                BlitVertex::new(vec3(1.0, -1.0, 0.0), [1.0, 1.0]),
                BlitVertex::new(vec3(1.0, 1.0, 0.0), [1.0, 0.0]),
                BlitVertex::new(vec3(-1.0, -1.0, 0.0), [0.0, 1.0]),
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        BlitState {
            sampler,
            bind_group_layout,
            bind_group,
            render_pipeline,
            vertex_buffer,
        }
    }
    fn resize(&mut self, device: &wgpu::Device, render_target_view: &wgpu::TextureView) {
        self.bind_group = Self::create_bind_group(
            device,
            &self.bind_group_layout,
            render_target_view,
            &self.sampler,
        );
    }
    fn create_bind_group(
        device: &wgpu::Device,
        bind_group_layout: &wgpu::BindGroupLayout,
        render_target_view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&render_target_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
            label: Some("blit_bind_group"),
        })
    }
    fn encode_draw_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        output_view: &wgpu::TextureView,
    ) {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });
        rpass.set_pipeline(&self.render_pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        rpass.draw(0..6, 0..1);
    }
}

struct Texture {
    #[allow(dead_code)]
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
impl Texture {
    fn new_rt_texture(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        texture_format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Render Target Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: texture_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: NonZeroU32::new(VIEW_COUNT),
            ..Default::default()
        });
        Self { texture, view }
    }
    fn new_depth_texture(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            array_layer_count: NonZeroU32::new(VIEW_COUNT),
            ..Default::default()
        });
        Self { texture, view }
    }
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
    fn to_view_proj_matrices(&self) -> Vec<f32> {
        let ipd = 68.3 / 1_000.0;
        let offset = vec4(ipd / 2.0, 0.0, 0.0, 0.0);

        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_rh(self.fov_y_rad, self.aspect_ratio, self.z_near, self.z_far);

        let mut view_l = view;
        view_l.w_axis += view * -offset;

        let mut view_r = view;
        view_r.w_axis += view * offset;
        [
            (proj * view_l).to_cols_array(),
            (proj * view_r).to_cols_array(),
        ]
        .concat()
    }
}
