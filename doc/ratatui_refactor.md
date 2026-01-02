# Refactoring Smallvil Ratatui Backend

## Goal
Refactor `smallvil` to leverage `GlesRenderer` for window composition while maintaining the existing `RatatuiBackend` for input/resize events and `GpuAnsiEncoder` for terminal output.

## Architecture Overview

The proposed data flow is as follows:
1.  **Input/State**: `RatatuiBackend` continues to handle `crossterm` input events and terminal resizing.
2.  **Composition**: `GlesRenderer` composes Wayland surfaces (windows) into an offscreen buffer.
3.  **Buffer Sharing**: The offscreen buffer is allocated as a `Dmabuf` (via `gbm`) to allow zero-copy sharing between GL (renderer) and Vulkan (WGPU/Encoder).
4.  **Encoding**: The `Dmabuf` is imported into `WGPU` (Vulkan backend) as a texture. `GpuAnsiEncoder` computes the ANSI difference sequence.
5.  **Output**: The resulting ANSI string is written to `stdout`.

## Detailed Design

### 1. Dependencies
Add the following dependencies to `smallvil/Cargo.toml`:
-   `gbm`: For buffer allocation (using `GbmAllocator`).
-   `drm`: To open the render node (needed for `gbm`).
-   `smithay` features: Ensure `backend_gbm` and `renderer_gles` are enabled.

### 2. Initialization (`init_ratatui`)

The initialization process in `smallvil/src/ratatui.rs` needs to be updated:

1.  **Open DRM Node**: Open a render node (e.g., `/dev/dri/renderD128`) to create a `GbmDevice`.
2.  **Setup Allocator**: Initialize `GbmAllocator<File>` with the DRM node.
3.  **Setup EGL/GL**:
    -   Create an `EGLDisplay` (likely using `gbm` platform or `surfaceless`).
    -   Initialize `GlesRenderer` using this EGL context.
4.  **Setup WGPU**:
    -   Continue using `wgpu::Instance` with `Vulkan` backend.
    -   Initialize `GpuAnsiEncoder` with the WGPU device/queue.

### 3. Rendering Logic

The render loop (in `RatatuiHandler::redraw`) will be transformed:

1.  **Buffer Allocation**:
    -   Instead of creating a `wgpu::Texture` directly, allocate a `Dmabuf` using `GbmAllocator`.
    -   The buffer size should match the terminal window size (scaled).
    -   Manage a small pool of buffers (double buffering) if necessary, or just one if we await encoding.

2.  **Composition (GL)**:
    -   Bind the allocated `Dmabuf` to `GlesRenderer` using `renderer.bind(dmabuf)`.
    -   Call `smithay::desktop::space::render_output` using the `GlesRenderer`.
    -   This renders the composite of all windows into the DMA-BUF.
    -   Unbind/Flush GL to ensure data is written.

3.  **Interop (Import to WGPU)**:
    -   Export the `Dmabuf` file descriptor (prime fd).
    -   Import this FD into `WGPU` as a `wgpu::Texture`.
    -   *Note*: This requires using `wgpu`'s unsafe HAL APIs or extensions (e.g., `wgpu::hal::vulkan::Device::create_texture_from_hal`) since standard WGPU doesn't expose DMA-BUF import directly in the high-level API yet (unless `smithay` provides a specific helper). We may need to adapt `smithay::backend::renderer::wgpu::import_dmabuf` logic or similar.

4.  **Encoding (GpuAnsiEncoder)**:
    -   Pass the imported `wgpu::Texture` to `GpuAnsiEncoder::ansi_from_texture`.
    -   The previous texture (for diffing) also needs to be maintained.

5.  **Output**:
    -   Write the resulting ANSI codes to `stdout` (buffering recommended).

### 4. Input Handling
`RatatuiBackend` and `RatatuiEventSource` remain effectively unchanged. They provide the heartbeat (timer) for redraws and translate `crossterm` events into Smithay input events.

### 5. Challenges & Mitigations
-   **WGPU DMA-BUF Import**: This is the most complex part. We relies on `wgpu-hal` and Vulkan extensions (`VK_EXT_external_memory_dma_buf`).
-   **Synchronization**: We must ensure GL has finished rendering before WGPU reads. `glFinish()` or EGL fences might be required before importing/dispatching the compute shader.
-   **Context Management**: `GlesRenderer` needs an active EGL context. Ensure we make it current before rendering.

## Verification
1.  **Build**: Verify `smallvil` compiles with new dependencies.
2.  **Run**: Launch `smallvil` in a terminal.
3.  **Visual**: Check that usage of `GlesRenderer` correctly composes windows (e.g. `weston-terminal` appears).
4.  **Performance**: Check FPS and ensure offscreen rendering isn't causing excessive CPU usage (the goal of using GPU encoder).
