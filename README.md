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

## Rendering flow

The code renders three instances of a triangle (two being the controllers) to a multi-view render target.

- In desktop mode, this render target is then blitted to the swapchain, and the user can select which view
  to look at using the arrow keys.
- In desktop with XR resolution mode, much the same occurs, except the window is resized to the XR headset's
  render resolution.
- In XR mode, the program synchronises with the headset and blits the multi-view render target to the
  headset as well.

Rendering to a render target is necessary to accommodate these:

- Showing what the user is seeing within the desktop window, without having to re-render the scene
- Decoupling the colour formats of the various display surfaces; wgpu (on my machine) will offer
  `BGRA8888`, while my OpenXR runtime offers `RGBA8888`.

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
