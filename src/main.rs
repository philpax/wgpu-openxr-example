use std::path::Path;

use anyhow::Context;
use glam::{vec3, vec4, Quat, Vec3};
use wgpu::util::DeviceExt;
use winit::{
    event::{ElementState, Event, KeyboardInput, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

#[cfg(feature = "xr")]
mod xr;

mod blit_state;
mod camera;
mod main_state;
mod texture;
mod types;

pub mod wgsl;

use blit_state::BlitState;
use camera::CameraState;
use main_state::{Instance, MainState};
use texture::Texture;
use types::*;

#[allow(dead_code)]
pub struct WgpuState {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn main() -> anyhow::Result<()> {
    use clap::{command, Parser};

    const MAIN_TRIANGLE_SCALE: f32 = 1.0;
    const HAND_TRIANGLE_SCALE: f32 = 0.1;

    #[derive(Parser, PartialEq)]
    #[command(author, version, about)]
    enum Args {
        /// Only desktop
        Desktop,
        /// Desktop with XR initialization and resolution
        DesktopWithXrResolution,
        /// Render to headset
        Xr,
    }

    #[cfg(feature = "xr")]
    let args = Args::parse();

    let wgpu_features = wgpu::Features::MULTIVIEW | wgpu::Features::PUSH_CONSTANTS;
    let wgpu_limits = wgpu::Limits {
        max_push_constant_size: 4,
        ..Default::default()
    };

    let event_loop = EventLoop::new();
    let window = winit::window::Window::new(&event_loop)?;

    #[cfg(feature = "xr")]
    let (wgpu_state, surface, mut xr_state) = if args != Args::Desktop {
        let (wgpu_state, xr_state) = xr::XrState::initialize_with_wgpu(wgpu_features, wgpu_limits)?;
        window.set_resizable(false);
        let view = xr_state.views()[0];
        window.set_inner_size(winit::dpi::PhysicalSize::new(
            view.recommended_image_rect_width,
            view.recommended_image_rect_height,
        ));
        let surface = unsafe { wgpu_state.instance.create_surface(&window) };
        (wgpu_state, surface, Some(xr_state))
    } else {
        let (wgpu_state, surface) = create_wgpu_state(&window, wgpu_features, wgpu_limits)?;
        (wgpu_state, surface, None)
    };

    #[cfg(not(feature = "xr"))]
    let (wgpu_state, surface) = create_wgpu_state(&window, wgpu_features, wgpu_limits)?;

    let mut camera_state = CameraState::new(&wgpu_state.device, window.inner_size());

    let preprocessor = wgsl::Preprocessor::from_directory(Path::new("shaders"))?;

    let window_swapchain_format = surface.get_supported_formats(&wgpu_state.adapter)[0];
    let mut main_state = MainState::new(
        &wgpu_state.device,
        &preprocessor,
        &camera_state,
        window_swapchain_format,
        vec![
            Instance::new(
                vec3(0.0, 0.0, 1.0),
                Quat::IDENTITY,
                Vec3::ONE * MAIN_TRIANGLE_SCALE,
            ),
            Instance::new(
                vec3(1.0, 0.0, 2.0),
                Quat::IDENTITY,
                Vec3::ONE * HAND_TRIANGLE_SCALE,
            ),
            Instance::new(
                vec3(-1.0, 0.0, 2.0),
                Quat::IDENTITY,
                Vec3::ONE * HAND_TRIANGLE_SCALE,
            ),
        ],
    );

    let mut config = {
        let size = window.inner_size();
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: window_swapchain_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Immediate,
        }
    };
    surface.configure(&wgpu_state.device, &config);
    let mut depth_texture = Texture::new_depth_texture(&wgpu_state.device, &config);
    let mut rt_texture =
        Texture::new_rt_texture(&wgpu_state.device, &config, window_swapchain_format);
    let mut blit_state = BlitState::new(
        &wgpu_state.device,
        &preprocessor,
        rt_texture.view(),
        window_swapchain_format,
        #[cfg(not(feature = "xr"))]
        window_swapchain_format,
        #[cfg(feature = "xr")]
        xr::WGPU_COLOR_FORMAT,
    );

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
    let mut view_index = 0;
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
                rt_texture =
                    Texture::new_rt_texture(&wgpu_state.device, &config, window_swapchain_format);

                blit_state.resize(&wgpu_state.device, rt_texture.view());
                camera_state.data.resize(size);

                // On macos the window needs to be redrawn manually after resizing
                window.request_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                virtual_keycode: Some(VirtualKeyCode::Left | VirtualKeyCode::Right),
                                state: ElementState::Released,
                                ..
                            },
                        ..
                    },
                ..
            } => {
                view_index = (view_index + 1) % 2;
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                virtual_keycode: Some(VirtualKeyCode::Escape),
                                state: ElementState::Released,
                                ..
                            },
                        ..
                    },
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared => {
                window.request_redraw();
                cleared = true;
            }
            _ => {}
        }

        if !cleared {
            return;
        }

        #[cfg(feature = "xr")]
        let xr_frame_state = if args == Args::Xr {
            xr_state.as_mut().and_then(|x| x.pre_frame().unwrap())
        } else {
            None
        };

        let mut encoder = wgpu_state
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        main_state.encode_draw_pass(
            &mut encoder,
            rt_texture.view(),
            depth_texture.view(),
            &triangle_vertex_buffer,
            camera_state.bind_group(),
        );

        let frame = surface
            .get_current_texture()
            .expect("Failed to acquire next swap chain texture");
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        blit_state.encode_draw_pass(&mut encoder, &view, Some(view_index));

        #[cfg(feature = "xr")]
        let pfd = xr_state
            .as_mut()
            .zip(xr_frame_state)
            .map(|(xr_state, xr_frame_state)| {
                xr_state
                    .post_frame(
                        &wgpu_state.device,
                        xr_frame_state,
                        &mut encoder,
                        &blit_state,
                    )
                    .unwrap()
            });

        let time_since_start = start_time.elapsed().as_secs_f32();
        {
            let insts = &mut main_state.instances;
            insts[0].rotation = Quat::from_rotation_y(time_since_start / std::f32::consts::PI);
            #[cfg(feature = "xr")]
            if let Some(pfd) = &pfd {
                if let Some(lh) = pfd.left_hand {
                    (insts[1].translation, insts[1].rotation) = lh;
                }
                if let Some(rh) = pfd.right_hand {
                    (insts[2].translation, insts[2].rotation) = rh;
                }
            }
        }
        main_state.upload_instances(&wgpu_state.queue);

        wgpu_state.queue.write_buffer(
            camera_state.buffer(),
            0,
            bytemuck::cast_slice(&{
                #[cfg(feature = "xr")]
                match &pfd {
                    Some(pfd) => camera_state
                        .data
                        .to_view_proj_matrices_with_xr_views(&pfd.views),
                    None => camera_state.data.to_view_proj_matrices(),
                }
                #[cfg(not(feature = "xr"))]
                camera_state.data.to_view_proj_matrices()
            }),
        );

        wgpu_state.queue.submit(Some(encoder.finish()));

        #[cfg(feature = "xr")]
        if let (Some(xr_state), Some(xr_frame_state), Some(pfd)) =
            (xr_state.as_mut(), xr_frame_state, &pfd)
        {
            xr_state
                .post_queue_submit(xr_frame_state, &pfd.views)
                .unwrap();
        }

        frame.present();

        fps_count += 1;
        if fps_timer.elapsed().as_millis() > 100 {
            window.set_title(&format!(
                "wgpu-openxr-example: {:.02} FPS | {} view",
                (fps_count as f32) / fps_timer.elapsed().as_secs_f32(),
                if view_index == 0 { "left" } else { "right" }
            ));

            fps_count = 0;
            fps_timer = std::time::Instant::now();
        }
    });
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
