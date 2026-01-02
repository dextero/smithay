# WgpuRenderer Design

This document outlines the design for a `WgpuRenderer` in Smithay, leveraging the `wgpu` crate for cross-platform, high-performance rendering.

## Goals

- Implement the `smithay::backend::renderer::Renderer` trait.
- Support importing Wayland buffers (SHM and DmaBuf).
- Provide a modern, efficient rendering pipeline.
- Enable easy integration with `smithay::desktop::space::render_output`.

## Architecture

### 1. Core Structures

- **`WgpuRenderer`**: The main struct implementing the `Renderer` trait. It owns the `wgpu::Device`, `wgpu::Queue`, and various caches (pipelines, bind group layouts, samplers).
- **`WgpuTexture`**: Implements the `Texture` trait. Wraps a `wgpu::Texture` and its `wgpu::TextureView`.
- **`WgpuFrame`**: Implements the `Frame` trait. Manages the lifetime of a single frame's `wgpu::CommandEncoder` and `wgpu::RenderPass`.

### 2. Renderer Trait Implementation

The `WgpuRenderer` will implement:
- `Renderer`: Basic rendering operations (clear, render).
- `ImportMem`: For importing SHM buffers.
- `ImportDma`: For importing DmaBufs (using `wgpu-hal` for Vulkan/EGL interop).
- `Bind<Target>`: To bind to various render targets (e.g., `wgpu::TextureView`).

### 3. Importing Buffers

#### SHM Buffers (`ImportMem`)
SHM buffers are uploaded to `wgpu::Texture`s using `wgpu::Queue::write_texture`.

#### DmaBufs (`ImportDma`)
Importing DmaBufs requires platform-specific extensions.
- **Vulkan**: Use `wgpu-hal`'s `create_texture_from_raw` after importing the DmaBuf into a Vulkan `VkImage` using `VK_EXT_external_memory_dma_buf` and `VK_EXT_image_drm_format_modifier`.
- **Other APIs**: Fallback to SHM if direct import is not available.

### 4. Render Pipeline

The renderer will use a specialized shader for compositing:
- **Vertex Shader**: Handles coordinate transformations (logical to physical, output rotations).
- **Fragment Shader**: Handles alpha blending, texture filtering, and color space conversions.

#### Coordinate Systems
The renderer must correctly handle:
- **Logical Coordinates**: Used by Smithay's desktop management.
- **Physical Coordinates**: Used for final rendering to the output.
- **Buffer Coordinates**: Used for sampling from textures.

### 5. Integration with `render_output`

By implementing the `Renderer` trait and providing a `WaylandSurfaceRenderElement` compatible with `WgpuRenderer`, compositors can use the standard `smithay::desktop::space::render_output` function for scene composition.

## Implementation Details

### Texture Caching
`WgpuRenderer` should maintain a cache of `WgpuTexture`s to avoid expensive re-imports of buffers that haven't changed. Smithay's `RendererSurfaceState` can be used to track buffer age and changes.

### Synchronization
Use `wgpu::Queue::submit` and potentially `wgpu::Device::poll` to manage GPU-CPU synchronization. Implement Smithay's `SyncPoint` using `wgpu`'s internal fencing or completion callbacks.
