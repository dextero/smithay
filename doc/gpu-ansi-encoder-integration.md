# Replacing Ratatui Backend with GPU ANSI Encoder

This document describes how to replace the existing Ratatui-based terminal output logic in `smallvil` with the GPU-accelerated ANSI encoder from `rust-gpu-test`, utilizing Vulkan for efficient DMA-BUF import.

## Existing Logic

The current implementation in `smallvil/src/ratatui.rs` uses Smithay's `RatatuiRenderer` (`src/backend/renderer/ratatui.rs`). 

### Key steps in the current logic:
1. **Initialization**: `init_ratatui` sets up a `RatatuiBackend`, which initializes a `ratatui::Terminal` with `crossterm`.
2. **DMA-BUF Import**: When a Wayland client provides a DMA-BUF, `RatatuiRenderer::import_dmabuf` performs a **CPU mapping** of the buffer using `dmabuf.map_plane`.
3. **CPU Processing**: The pixels are copied from the mapped memory into a `RatatuiTexture` (a `Vec` of ARGB pixels).
4. **Rendering**: `smithay::desktop::space::render_output` calls `RatatuiFrame::render_texture_from_to`. This function samples pixels and updates a `ratatui::buffer::Buffer` in software.
5. **Output**: `RatatuiFrame::drop` triggers `terminal.draw()`, which uses `ratatui` and `crossterm` to write ANSI sequences to `stdout`.

## Proposed Logic with `GpuAnsiEncoder` (Vulkan-based)

The `GpuAnsiEncoder` from `rust-gpu-test` moves the frame comparison and ANSI sequence generation to the GPU using compute shaders. We will use Vulkan directly to avoid any dependency on EGL.

### Key steps in the new logic:
1. **WGPU & Vulkan Integration**:
    - Initialize `wgpu` with the **Vulkan backend**.
    - Ensure the Vulkan device is created with necessary extensions:
        - `VK_EXT_external_memory_dma_buf`
        - `VK_KHR_external_memory_fd`
        - `VK_EXT_image_drm_format_modifier`
    - Create a `GpuAnsiEncoder` instance: `GpuAnsiEncoder::new(device, queue).await`.
2. **Vulkan DMA-BUF Import**:
    - Instead of CPU mapping, import the DMA-BUF file descriptor directly into a Vulkan `VkImage`.
    - Use `VkExternalMemoryImageCreateInfoKHR` and `VkImportMemoryFdInfoKHR` to bind the DMA-BUF memory to the Vulkan image.
3. **Bridge to WGPU**:
    - Use `wgpu`'s Hardware Abstraction Layer (`wgpu-hal`) to wrap the raw Vulkan `VkImage` handle into a `wgpu::Texture`.
    - This allows `GpuAnsiEncoder` to process the texture as if it were a native `wgpu` resource, without any CPU-side pixel copies.
4. **GPU-Accelerated Encoding**:
    - For each frame, call `encoder.ansi_from_texture(previous_texture, current_texture).await`.
    - The encoder computes differences and generates ANSI strings directly on the GPU.
5. **Direct Output**:
    - Print the resulting `String` directly to `stdout`. This bypasses `ratatui` entirely for the rendering path.

## Benefits
- **Zero CPU pixel manipulation**: No manual loops or software blending.
- **No EGL Dependency**: Uses pure Vulkan for hardware-accelerated buffer sharing.
- **Efficient Frame Differencing**: Only regions that changed are sent as ANSI sequences, significantly reducing TTY bandwidth.
- **True Zero-Copy**: Pixels stay in GPU memory from the Wayland client's buffer all the way through the ANSI encoding process.
