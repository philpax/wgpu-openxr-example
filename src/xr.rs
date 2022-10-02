use std::ffi::c_void;

use anyhow::Context;
use ash::vk::{self, Handle};
use openxr as xr;

use crate::WgpuState;

pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;
const VIEW_TYPE: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

pub struct XrState {
    xr_instance: xr::Instance,
    environment_blend_mode: xr::EnvironmentBlendMode,
    session: xr::Session<xr::Vulkan>,
    session_running: bool,
    frame_wait: xr::FrameWaiter,
    frame_stream: xr::FrameStream<xr::Vulkan>,
    action_set: xr::ActionSet,
    right_action: xr::Action<xr::Posef>,
    left_action: xr::Action<xr::Posef>,
    right_space: xr::Space,
    left_space: xr::Space,
    stage: xr::Space,
    event_storage: xr::EventDataBuffer,
}
impl XrState {
    pub fn initialize_with_wgpu(
        wgpu_features: wgpu::Features,
        wgpu_limits: wgpu::Limits,
    ) -> anyhow::Result<(WgpuState, XrState)> {
        use wgpu_hal::{api::Vulkan as V, Api};

        let entry = xr::Entry::linked();
        let available_extensions = entry.enumerate_extensions()?;
        assert!(available_extensions.khr_vulkan_enable2);
        let mut enabled_extensions = xr::ExtensionSet::default();
        enabled_extensions.khr_vulkan_enable2 = true;
        #[cfg(target_os = "android")]
        {
            enabled_extensions.khr_android_create_instance = true;
        }
        let xr_instance = entry.create_instance(
            &xr::ApplicationInfo {
                application_name: "wgpu-openxr-example",
                application_version: 0,
                engine_name: "wgpu-openxr-example",
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
        let vk_target_version = vk::make_api_version(0, 1, 1, 0);
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
            xr_instance.vulkan_graphics_device(xr_system_id, vk_instance.handle().as_raw() as _)?
                as _
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

        let (wgpu_open_device, vk_device_ptr, queue_family_index) =
            {
                let uab_types = wgpu_hal::UpdateAfterBindTypes::from_limits(
                    &wgpu_limits,
                    &wgpu_exposed_adapter
                        .adapter
                        .physical_device_capabilities()
                        .properties()
                        .limits,
                );

                let mut enabled_phd_features = wgpu_exposed_adapter
                    .adapter
                    .physical_device_features(&enabled_extensions, wgpu_features, uab_types);
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

        let (session, frame_wait, frame_stream) = unsafe {
            xr_instance.create_session::<xr::Vulkan>(
                xr_system_id,
                &xr::vulkan::SessionCreateInfo {
                    instance: vk_instance_ptr,
                    physical_device: vk_physical_device_ptr,
                    device: vk_device_ptr,
                    queue_family_index: queue_family_index,
                    queue_index: 0,
                },
            )
        }?;
        let action_set = xr_instance.create_action_set("input", "input pose information", 0)?;
        let right_action =
            action_set.create_action::<xr::Posef>("right_hand", "Right Hand Controller", &[])?;
        let left_action =
            action_set.create_action::<xr::Posef>("left_hand", "Left Hand Controller", &[])?;
        xr_instance.suggest_interaction_profile_bindings(
            xr_instance.string_to_path("/interaction_profiles/khr/simple_controller")?,
            &[
                xr::Binding::new(
                    &right_action,
                    xr_instance.string_to_path("/user/hand/right/input/grip/pose")?,
                ),
                xr::Binding::new(
                    &left_action,
                    xr_instance.string_to_path("/user/hand/left/input/grip/pose")?,
                ),
            ],
        )?;
        session.attach_action_sets(&[&action_set])?;
        let right_space =
            right_action.create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)?;
        let left_space =
            left_action.create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)?;
        let stage =
            session.create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)?;
        Ok((
            WgpuState {
                instance: wgpu_instance,
                adapter: wgpu_adapter,
                device: wgpu_device,
                queue: wgpu_queue,
            },
            XrState {
                xr_instance,
                environment_blend_mode,
                session,
                session_running: false,
                frame_wait,
                frame_stream,
                action_set,
                right_action,
                left_action,
                right_space,
                left_space,
                stage,
                event_storage: xr::EventDataBuffer::new(),
            },
        ))
    }

    pub fn pre_frame(&mut self) -> anyhow::Result<Option<xr::FrameState>> {
        while let Some(event) = self.xr_instance.poll_event(&mut self.event_storage)? {
            use xr::Event::*;
            match event {
                SessionStateChanged(e) => {
                    // Session state change is where we can begin and end sessions, as well as
                    // find quit messages!
                    println!("entered state {:?}", e.state());
                    match e.state() {
                        xr::SessionState::READY => {
                            self.session.begin(VIEW_TYPE)?;
                            self.session_running = true;
                        }
                        xr::SessionState::STOPPING => {
                            self.session.end()?;
                            self.session_running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            return Ok(None);
                        }
                        _ => {}
                    }
                }
                InstanceLossPending(_) => {
                    return Ok(None);
                }
                EventsLost(e) => {
                    println!("lost {} events", e.lost_event_count());
                }
                _ => {}
            }
        }
        if !self.session_running {
            // Don't grind up the CPU
            std::thread::sleep(std::time::Duration::from_millis(10));
            return Ok(None);
        }

        // Block until the previous frame is finished displaying, and is ready for another one.
        // Also returns a prediction of when the next frame will be displayed, for use with
        // predicting locations of controllers, viewpoints, etc.
        let xr_frame_state = self.frame_wait.wait()?;
        // Must be called before any rendering is done!
        self.frame_stream.begin()?;

        Ok(Some(xr_frame_state))
    }

    pub fn post_frame(&mut self, xr_frame_state: xr::FrameState) -> anyhow::Result<()> {
        if !xr_frame_state.should_render {
            self.frame_stream.end(
                xr_frame_state.predicted_display_time,
                self.environment_blend_mode,
                &[],
            )?;
            return Ok(());
        }
        self.session.sync_actions(&[(&self.action_set).into()])?;
        let right_location = self
            .right_space
            .locate(&self.stage, xr_frame_state.predicted_display_time)?;
        let left_location = self
            .left_space
            .locate(&self.stage, xr_frame_state.predicted_display_time)?;
        let mut printed = false;
        if self.left_action.is_active(&self.session, xr::Path::NULL)? {
            print!(
                "Left Hand: ({:0<12},{:0<12},{:0<12}), ",
                left_location.pose.position.x,
                left_location.pose.position.y,
                left_location.pose.position.z
            );
            printed = true;
        }
        if self.right_action.is_active(&self.session, xr::Path::NULL)? {
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
        let (_, _views) = self.session.locate_views(
            VIEW_TYPE,
            xr_frame_state.predicted_display_time,
            &self.stage,
        )?;
        self.frame_stream.end(
            xr_frame_state.predicted_display_time,
            self.environment_blend_mode,
            &[],
        )?;
        Ok(())
    }
}
