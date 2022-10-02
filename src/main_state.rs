use glam::{vec3, Mat4, Quat, Vec3};
use std::{borrow::Cow, num::NonZeroU32};
use wgpu::util::DeviceExt;

use crate::{
    camera::CameraState,
    types::{Vertex, DEPTH_FORMAT, VIEW_COUNT},
};

pub struct MainState {
    #[allow(dead_code)]
    shader: wgpu::ShaderModule,
    #[allow(dead_code)]
    pipeline_layout: wgpu::PipelineLayout,
    pipeline: wgpu::RenderPipeline,
    pub instances: [(Vec3, Quat); 3],
    instance_buffer: wgpu::Buffer,
}
impl MainState {
    pub fn new(
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
            bind_group_layouts: &[&camera_state.bind_group_layout()],
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
    pub fn upload_instances(&self, queue: &wgpu::Queue) {
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
    pub fn encode_draw_pass(
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
