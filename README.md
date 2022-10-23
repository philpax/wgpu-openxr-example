# wgpu-openxr-example

a barebones example of how to integrate OpenXR with wgpu (Vulkan-only)

It has four modes:

- `cargo run --no-default-features`: desktop-only, renders the scene without _any_ XR integration
- `cargo run -- desktop`: build with XR support, but render the scene without initialising XR
- `cargo run -- desktop-with-xr-resolution`: build with XR support, initialise XR, but do not render to headset
- `cargo run -- xr`: build with XR support, and render to the headset

These modes are intended to show you how to gracefully integrate XR into your project's code
and how you can move from one stage of integration to the next.

Note that this code is not production-quality; there are a few shortcuts that have been taken
in the interest of keeping it simple and relatively modular. Make sure to clean up your resources
properly and use robust code where possible :)

## Future

It should theoretically be possible to do the following:

- D3D11/12 backend with OpenXR
- WebGL2 backend with WebXR
- Metal backend with ARKit

## Reference

Cobbled together from the following sources:

- <https://github.com/gfx-rs/wgpu-rs/blob/7501ba1311c45c57cf4e361556ab46431478adb4/examples/hello-vr-triangle/main.rs>
- <https://github.com/gfx-rs/wgpu/blob/b65ebb4b308228287cdb559b624ce3ac3ba71bd2/wgpu/examples/hello-triangle/main.rs>
- <https://github.com/Ralith/openxrs/blob/a6a7d9c00afc5b8d9aa9eb19c692aaf1a7fa6d16/openxr/examples/vulkan.rs>
- <https://github.com/gfx-rs/wgpu/blob/b65ebb4b308228287cdb559b624ce3ac3ba71bd2/wgpu-hal/src/vulkan/adapter.rs>
- <https://github.com/gfx-rs/wgpu/blob/b65ebb4b308228287cdb559b624ce3ac3ba71bd2/wgpu-hal/src/vulkan/instance.rs>
