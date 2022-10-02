use glam::{vec3, vec4, Mat4, Vec3};
use wgpu::util::DeviceExt;

pub struct PerspectiveCamera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,

    pub aspect_ratio: f32,
    pub fov_y_rad: f32,
    pub z_near: f32,
    pub z_far: f32,
}
impl PerspectiveCamera {
    pub fn to_view_proj_matrices(&self) -> Vec<f32> {
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
    pub fn resize(&mut self, inner_size: winit::dpi::PhysicalSize<u32>) {
        self.aspect_ratio = inner_size.width as f32 / inner_size.height as f32;
    }
}

pub struct CameraState {
    pub data: PerspectiveCamera,

    buffer: wgpu::Buffer,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
}
impl CameraState {
    pub fn new(device: &wgpu::Device, inner_size: winit::dpi::PhysicalSize<u32>) -> Self {
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
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.bind_group
    }
    pub fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bind_group_layout
    }
}
