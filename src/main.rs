use anyhow::Context;
use ash::vk::{self, Handle};
use openxr as xr;
use std::{borrow::Cow, ffi::c_void};
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
pub const VIEW_COUNT: u32 = 2;
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

fn main() -> anyhow::Result<()> {
    let entry = xr::Entry::linked();
    let available_extensions = entry.enumerate_extensions()?;
    assert!(available_extensions.khr_vulkan_enable2);

    let wgpu_features = wgpu::Features::MULTIVIEW;
    let wgpu_limits = wgpu::Limits::default();

    // Initialize OpenXR with the extensions we've found!
    let mut enabled_extensions = xr::ExtensionSet::default();
    enabled_extensions.khr_vulkan_enable2 = true;
    #[cfg(target_os = "android")]
    {
        enabled_extensions.khr_android_create_instance = true;
    }
    let xr_instance = entry.create_instance(
        &xr::ApplicationInfo {
            application_name: "openxrs example",
            application_version: 0,
            engine_name: "openxrs example",
            engine_version: 0,
        },
        &enabled_extensions,
        &[],
    )?;
    let instance_props = xr_instance.properties()?;
    println!(
        "loaded OpenXR runtime: {} {}",
        instance_props.runtime_name, instance_props.runtime_version
    );

    let xr_system_id = xr_instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)?;

    let environment_blend_mode =
        xr_instance.enumerate_environment_blend_modes(xr_system_id, VIEW_TYPE)?[0];

    let vk_target_version = vk::make_api_version(0, 1, 1, 0); // Vulkan 1.1 guarantees multiview support
    let vk_target_version_xr = xr::Version::new(1, 1, 0);

    let reqs = xr_instance.graphics_requirements::<xr::Vulkan>(xr_system_id)?;

    if vk_target_version_xr < reqs.min_api_version_supported
        || vk_target_version_xr.major() > reqs.max_api_version_supported.major()
    {
        panic!(
            "OpenXR runtime requires Vulkan version > {}, < {}.0.0",
            reqs.min_api_version_supported,
            reqs.max_api_version_supported.major() + 1
        );
    }

    let GpuState {
        instance,
        adapter,
        device,
        queue,
        vk_instance_ptr,
        vk_physical_device_ptr,
        vk_device_ptr,
        queue_family_index,
    } = create_wgpu_state_from_xr(
        &xr_instance,
        xr_system_id,
        vk_target_version,
        wgpu_features,
        wgpu_limits,
    )?;

    let event_loop = EventLoop::new();
    let window = winit::window::Window::new(&event_loop)?;

    let size = window.inner_size();
    let surface = unsafe { instance.create_surface(&window) };

    // Load the shaders from disk
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        push_constant_ranges: &[],
    });

    let swapchain_format = surface.get_supported_formats(&adapter)[0];

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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

    surface.configure(&device, &config);

    // A session represents this application's desire to display things! This is where we hook
    // up our graphics API. This does not start the session; for that, you'll need a call to
    // Session::begin, which we do in 'main_loop below.
    let (session, mut frame_wait, mut frame_stream) = unsafe {
        xr_instance.create_session::<xr::Vulkan>(
            xr_system_id,
            &xr::vulkan::SessionCreateInfo {
                instance: vk_instance_ptr,
                physical_device: vk_physical_device_ptr,
                device: vk_device_ptr,
                queue_family_index,
                queue_index: 0,
            },
        )
    }?;

    // Create an action set to encapsulate our actions
    let action_set = xr_instance.create_action_set("input", "input pose information", 0)?;

    let right_action =
        action_set.create_action::<xr::Posef>("right_hand", "Right Hand Controller", &[])?;
    let left_action =
        action_set.create_action::<xr::Posef>("left_hand", "Left Hand Controller", &[])?;

    // Bind our actions to input devices using the given profile
    // If you want to access inputs specific to a particular device you may specify a different
    // interaction profile
    xr_instance.suggest_interaction_profile_bindings(
        xr_instance
            .string_to_path("/interaction_profiles/khr/simple_controller")
            .unwrap(),
        &[
            xr::Binding::new(
                &right_action,
                xr_instance
                    .string_to_path("/user/hand/right/input/grip/pose")
                    .unwrap(),
            ),
            xr::Binding::new(
                &left_action,
                xr_instance
                    .string_to_path("/user/hand/left/input/grip/pose")
                    .unwrap(),
            ),
        ],
    )?;

    // Attach the action set to the session
    session.attach_action_sets(&[&action_set]).unwrap();

    // Create an action space for each device we want to locate
    let right_space = right_action
        .create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)
        .unwrap();
    let left_space = left_action
        .create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)
        .unwrap();

    // OpenXR uses a couple different types of reference frames for positioning content; we need
    // to choose one for displaying our content! STAGE would be relative to the center of your
    // guardian system's bounds, and LOCAL would be relative to your device's starting location.
    let stage = session
        .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
        .unwrap();

    let mut event_storage = xr::EventDataBuffer::new();
    let mut session_running = false;
    event_loop.run(move |event, _, control_flow| {
        // Have the closure take ownership of the resources.
        // `event_loop.run` never returns, therefore we must do this to ensure
        // the resources are properly cleaned up.
        let _ = (&instance, &adapter, &shader, &pipeline_layout);

        while let Some(event) = xr_instance.poll_event(&mut event_storage).unwrap() {
            use xr::Event::*;
            match event {
                SessionStateChanged(e) => {
                    // Session state change is where we can begin and end sessions, as well as
                    // find quit messages!
                    println!("entered state {:?}", e.state());
                    match e.state() {
                        xr::SessionState::READY => {
                            session.begin(VIEW_TYPE).unwrap();
                            session_running = true;
                        }
                        xr::SessionState::STOPPING => {
                            session.end().unwrap();
                            session_running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            return;
                        }
                        _ => {}
                    }
                }
                InstanceLossPending(_) => {
                    return;
                }
                EventsLost(e) => {
                    println!("lost {} events", e.lost_event_count());
                }
                _ => {}
            }
        }

        #[cfg(feature = "xr")]
        if !session_running {
            // Don't grind up the CPU
            std::thread::sleep(std::time::Duration::from_millis(10));
            return;
        }

        #[cfg(feature = "xr")]
        let xr_frame_state = {
            // Block until the previous frame is finished displaying, and is ready for another one.
            // Also returns a prediction of when the next frame will be displayed, for use with
            // predicting locations of controllers, viewpoints, etc.
            let xr_frame_state = frame_wait.wait().unwrap();
            // Must be called before any rendering is done!
            frame_stream.begin().unwrap();
            xr_frame_state
        };

        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                // Reconfigure the surface with the new size
                config.width = size.width;
                config.height = size.height;
                surface.configure(&device, &config);
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
                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
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

                queue.submit(Some(encoder.finish()));
                frame.present();
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }

        #[cfg(feature = "xr")]
        {
            if !xr_frame_state.should_render {
                frame_stream
                    .end(
                        xr_frame_state.predicted_display_time,
                        environment_blend_mode,
                        &[],
                    )
                    .unwrap();
                return;
            }

            session.sync_actions(&[(&action_set).into()]).unwrap();

            // Find where our controllers are located in the Stage space
            let right_location = right_space
                .locate(&stage, xr_frame_state.predicted_display_time)
                .unwrap();

            let left_location = left_space
                .locate(&stage, xr_frame_state.predicted_display_time)
                .unwrap();

            let mut printed = false;
            if left_action.is_active(&session, xr::Path::NULL).unwrap() {
                print!(
                    "Left Hand: ({:0<12},{:0<12},{:0<12}), ",
                    left_location.pose.position.x,
                    left_location.pose.position.y,
                    left_location.pose.position.z
                );
                printed = true;
            }

            if right_action.is_active(&session, xr::Path::NULL).unwrap() {
                print!(
                    "Right Hand: ({:0<12},{:0<12},{:0<12})",
                    right_location.pose.position.x,
                    right_location.pose.position.y,
                    right_location.pose.position.z
                );
                printed = true;
            }
            if printed {
                println!();
            }

            // Fetch the view transforms. To minimize latency, we intentionally do this *after*
            // recording commands to render the scene, i.e. at the last possible moment before
            // rendering begins in earnest on the GPU. Uniforms dependent on this data can be sent
            // to the GPU just-in-time by writing them to per-frame host-visible memory which the
            // GPU will only read once the command buffer is submitted.
            let (_, _views) = session
                .locate_views(VIEW_TYPE, xr_frame_state.predicted_display_time, &stage)
                .unwrap();

            frame_stream
                .end(
                    xr_frame_state.predicted_display_time,
                    environment_blend_mode,
                    &[],
                )
                .unwrap();
        }
    });
}

struct GpuState {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    vk_instance_ptr: *const c_void,
    vk_physical_device_ptr: *const c_void,
    vk_device_ptr: *const c_void,
    queue_family_index: u32,
}

fn create_wgpu_state_from_xr(
    xr_instance: &xr::Instance,
    xr_system_id: xr::SystemId,
    vk_target_version: u32,
    wgpu_features: wgpu::Features,
    wgpu_limits: wgpu::Limits,
) -> anyhow::Result<GpuState> {
    use wgpu_hal::{api::Vulkan as V, Api};

    let vk_entry = unsafe { ash::Entry::load() }?;
    let flags = wgpu_hal::InstanceFlags::empty();
    let extensions = <V as Api>::Instance::required_extensions(&vk_entry, flags)?;

    let vk_instance = unsafe {
        let extensions_cchar: Vec<_> = extensions.iter().map(|s| s.as_ptr()).collect();

        let vk_app_info = vk::ApplicationInfo::builder()
            .application_version(0)
            .engine_version(0)
            .api_version(vk_target_version);

        let vk_instance = xr_instance
            .create_vulkan_instance(
                xr_system_id,
                std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                &vk::InstanceCreateInfo::builder()
                    .application_info(&vk_app_info)
                    .enabled_extension_names(&extensions_cchar) as *const _
                    as *const _,
            )
            .context("XR error creating Vulkan instance")?
            .map_err(vk::Result::from_raw)
            .context("Vulkan error creating Vulkan instance")?;

        ash::Instance::load(
            vk_entry.static_fn(),
            vk::Instance::from_raw(vk_instance as _),
        )
    };
    let vk_instance_ptr = vk_instance.handle().as_raw() as *const c_void;

    let vk_physical_device = vk::PhysicalDevice::from_raw(unsafe {
        xr_instance.vulkan_graphics_device(xr_system_id, vk_instance.handle().as_raw() as _)? as _
    });
    let vk_physical_device_ptr = vk_physical_device.as_raw() as *const c_void;

    let vk_device_properties =
        unsafe { vk_instance.get_physical_device_properties(vk_physical_device) };
    if vk_device_properties.api_version < vk_target_version {
        unsafe { vk_instance.destroy_instance(None) }
        panic!("Vulkan physical device doesn't support version 1.1");
    }

    let wgpu_vk_instance = unsafe {
        <V as Api>::Instance::from_raw(
            vk_entry.clone(),
            vk_instance.clone(),
            vk_target_version,
            0,
            extensions,
            flags,
            false,
            Some(Box::new(())),
        )?
    };
    let wgpu_exposed_adapter = wgpu_vk_instance
        .expose_adapter(vk_physical_device)
        .context("failed to expose adapter")?;

    let enabled_extensions = wgpu_exposed_adapter
        .adapter
        .required_device_extensions(wgpu_features);

    let (wgpu_open_device, vk_device_ptr, queue_family_index) = {
        let uab_types = wgpu_hal::UpdateAfterBindTypes::from_limits(
            &wgpu_limits,
            &wgpu_exposed_adapter
                .adapter
                .physical_device_capabilities()
                .properties()
                .limits,
        );

        let mut enabled_phd_features = wgpu_exposed_adapter.adapter.physical_device_features(
            &enabled_extensions,
            wgpu_features,
            uab_types,
        );
        let family_index = 0;
        let family_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(family_index)
            .queue_priorities(&[1.0])
            .build();
        let family_infos = [family_info];
        let info = enabled_phd_features
            .add_to_device_create_builder(
                vk::DeviceCreateInfo::builder()
                    .queue_create_infos(&family_infos)
                    .push_next(&mut vk::PhysicalDeviceMultiviewFeatures {
                        multiview: vk::TRUE,
                        ..Default::default()
                    }),
            )
            .build();
        let vk_device = unsafe {
            let vk_device = xr_instance
                .create_vulkan_device(
                    xr_system_id,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    vk_physical_device.as_raw() as _,
                    &info as *const _ as *const _,
                )
                .context("XR error creating Vulkan device")?
                .map_err(vk::Result::from_raw)
                .context("Vulkan error creating Vulkan device")?;

            ash::Device::load(vk_instance.fp_v1_0(), vk::Device::from_raw(vk_device as _))
        };
        let vk_device_ptr = vk_device.handle().as_raw() as *const c_void;

        let wgpu_open_device = unsafe {
            wgpu_exposed_adapter.adapter.device_from_raw(
                vk_device,
                true,
                &enabled_extensions,
                wgpu_features,
                uab_types,
                family_info.queue_family_index,
                0,
            )
        }?;

        (
            wgpu_open_device,
            vk_device_ptr,
            family_info.queue_family_index,
        )
    };

    let wgpu_instance =
        unsafe { wgpu::Instance::from_hal::<wgpu_hal::api::Vulkan>(wgpu_vk_instance) };
    let wgpu_adapter = unsafe { wgpu_instance.create_adapter_from_hal(wgpu_exposed_adapter) };
    let (wgpu_device, wgpu_queue) = unsafe {
        wgpu_adapter.create_device_from_hal(
            wgpu_open_device,
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu_features,
                limits: wgpu_limits.clone(),
            },
            None,
        )
    }?;

    Ok(GpuState {
        instance: wgpu_instance,
        adapter: wgpu_adapter,
        device: wgpu_device,
        queue: wgpu_queue,
        vk_instance_ptr,
        vk_physical_device_ptr,
        vk_device_ptr,
        queue_family_index,
    })
}
