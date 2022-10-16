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
        let ipd = 63.0 / 1_000.0;
        let offset = vec4(ipd / 2.0, 0.0, 0.0, 0.0);

        let view = Mat4::look_at_rh(self.eye, self.target, self.up);
        let proj = Mat4::perspective_rh(self.fov_y_rad, self.aspect_ratio, self.z_near, self.z_far);

        [-offset, offset]
            .map(|o| {
                let mut view = view;
                view.w_axis += view * o;
                (proj * view).to_cols_array()
            })
            .concat()
    }

    #[cfg(feature = "xr")]
    pub fn to_view_proj_matrices_with_xr_views(&self, views: &[openxr::View]) -> Vec<f32> {
        use glam::Quat;

        views
            .iter()
            .flat_map(|v| {
                let pose = v.pose;
                // with enough sign errors anything is possible
                let xr_rotation = {
                    let o = pose.orientation;
                    Quat::from_rotation_x(180.0f32.to_radians()) * glam::quat(o.w, o.z, o.y, o.x)
                };
                let xr_translation =
                    glam::vec3(-pose.position.x, pose.position.y, -pose.position.z);

                let view = Mat4::look_at_rh(
                    self.eye + xr_translation,
                    self.eye + xr_translation + xr_rotation * Vec3::Z,
                    xr_rotation * Vec3::Y,
                );

                let [tan_left, tan_right, tan_down, tan_up] = [
                    v.fov.angle_left,
                    v.fov.angle_right,
                    v.fov.angle_down,
                    v.fov.angle_up,
                ]
                .map(f32::tan);
                let tan_width = tan_right - tan_left;
                let tan_height = tan_up - tan_down;

                let a11 = 2.0 / tan_width;
                let a22 = 2.0 / tan_height;

                let a31 = (tan_right + tan_left) / tan_width;
                let a32 = (tan_up + tan_down) / tan_height;
                let a33 = -self.z_far / (self.z_far - self.z_near);

                let a43 = -(self.z_far * self.z_near) / (self.z_far - self.z_near);

                let proj = glam::Mat4::from_cols_array(&[
                    a11, 0.0, 0.0, 0.0, //
                    0.0, a22, 0.0, 0.0, //
                    a31, a32, a33, -1.0, //
                    0.0, 0.0, a43, 0.0, //
                ]);

                (proj * view).to_cols_array()
            })
            .collect()
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
