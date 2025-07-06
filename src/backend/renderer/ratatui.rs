#![cfg_attr(docsrs, doc(cfg(feature = "ratatui_backend")))]

use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use crossterm::ExecutableCommand;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;
use ratatui::prelude::CrosstermBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use tracing::warn;

use crate::backend::allocator::dmabuf::DmabufMappingMode;
use crate::backend::allocator::{Buffer, Fourcc};
use crate::backend::renderer::sync::Interrupted;
use crate::backend::renderer::{
    sync, Color32F, ContextId, DebugFlags, Frame, ImportDma, ImportDmaWl, ImportMemWl, InnerContextId,
    Renderer, RendererSuper, Texture, TextureFilter,
};
use crate::utils::{Buffer as BufferCoord, Physical, Rectangle, Size, Transform};

#[cfg(all(
    feature = "wayland_frontend",
    feature = "backend_egl",
    feature = "use_system_lib"
))]
use crate::backend::{egl::display::EGLBufferReader, renderer::ImportEgl};
use crate::wayland::shm::{shm_format_to_fourcc, with_buffer_contents};

/// A renderer for the ratatui backend
#[derive(Debug)]
pub struct RatatuiRenderer {
    /// TODO: docs
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl RatatuiRenderer {
    /// Create a new ratatui renderer
    pub fn new() -> Self {
        let terminal = ratatui::init();
        std::io::stdout().execute(crossterm::event::EnableMouseCapture).unwrap();
        Self { terminal }
    }

    /// Return the window size, in cells
    pub fn window_size(&self) -> Size<i32, Physical> {
        let size = self.terminal.size().unwrap();
        Size::new(size.width.into(), size.height.into())
    }

    /// TODO
    pub fn swap_buffers(&mut self, tex: RatatuiTexture) -> Result<RatatuiTexture, RatatuiError> {
        {
            let mut tex = tex.buffer.lock().unwrap();
            let expected_size = self.terminal.size().unwrap();
            let actual_size = tex.area().as_size();
            if expected_size != actual_size {
                return Err(RatatuiError::InvalidTextureSize {
                    actual: actual_size,
                    expected: expected_size,
                });
            }
            std::mem::swap(self.terminal.current_buffer_mut(), &mut tex);
        }
        Ok(tex)
    }
}

impl Drop for RatatuiRenderer {
    fn drop(&mut self) {
        let _ = std::io::stdout().execute(crossterm::event::DisableMouseCapture);
        ratatui::restore();
    }
}

impl ImportMemWl for RatatuiRenderer {
    fn import_shm_buffer(
        &mut self,
        buffer: &wayland_server::protocol::wl_buffer::WlBuffer,
        _surface: Option<&crate::wayland::compositor::SurfaceData>,
        _damage: &[Rectangle<i32, BufferCoord>],
    ) -> Result<Self::TextureId, Self::Error> {
        with_buffer_contents(buffer, |ptr, len, data| -> Result<Self::TextureId, Self::Error> {
            let size = Size::new(data.width.try_into().unwrap(), data.height.try_into().unwrap());
            let fourcc = shm_format_to_fourcc(data.format)
                .ok_or(RatatuiError::UnsupportedWlPixelFormat(data.format))?;
            let buf = match fourcc {
                Fourcc::Argb8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Argb8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Xrgb8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Xrgb8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Rgba8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Rgba8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Rgbx8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Rgbx8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Abgr8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Abgr8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Xbgr8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Xbgr8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Bgra8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Bgra8888 as _ }>(ptr as *const _, len, size)
                }
                Fourcc::Bgrx8888 => {
                    buffer_from_ptr_len::<{ Fourcc::Bgrx8888 as _ }>(ptr as *const _, len, size)
                }
                f => todo!("unsupported format: {f:?}"),
            };
            Ok(RatatuiTexture::from(buf))
        })?
    }
}

impl ImportDmaWl for RatatuiRenderer {}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Pixel<const F: u32>(u32);

impl<const F: u32> Pixel<F> {
    fn r(&self) -> u8 {
        match Fourcc::try_from(F) {
            Ok(Fourcc::Argb8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Xrgb8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Rgba8888) => (self.0 >> 24) as u8,
            Ok(Fourcc::Rgbx8888) => (self.0 >> 24) as u8,
            Ok(Fourcc::Abgr8888) => (self.0 >> 0) as u8,
            Ok(Fourcc::Xbgr8888) => (self.0 >> 0) as u8,
            Ok(Fourcc::Bgra8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Bgrx8888) => (self.0 >> 8) as u8,
            Ok(f) => todo!("unsupported format: {f:?}"),
            Err(e) => todo!("invalid format: {e:?}"),
        }
    }

    fn g(&self) -> u8 {
        match Fourcc::try_from(F) {
            Ok(Fourcc::Argb8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Xrgb8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Rgba8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Rgbx8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Abgr8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Xbgr8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Bgra8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Bgrx8888) => (self.0 >> 16) as u8,
            Ok(f) => todo!("unsupported format: {f:?}"),
            Err(e) => todo!("invalid format: {e:?}"),
        }
    }

    fn b(&self) -> u8 {
        match Fourcc::try_from(F) {
            Ok(Fourcc::Argb8888) => (self.0 >> 0) as u8,
            Ok(Fourcc::Xrgb8888) => (self.0 >> 0) as u8,
            Ok(Fourcc::Rgba8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Rgbx8888) => (self.0 >> 8) as u8,
            Ok(Fourcc::Abgr8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Xbgr8888) => (self.0 >> 16) as u8,
            Ok(Fourcc::Bgra8888) => (self.0 >> 24) as u8,
            Ok(Fourcc::Bgrx8888) => (self.0 >> 24) as u8,
            Ok(f) => todo!("unsupported format: {f:?}"),
            Err(e) => todo!("invalid format: {e:?}"),
        }
    }
}

impl<const F: u32> Into<Cell> for &Pixel<F> {
    fn into(self) -> Cell {
        let mut cell = Cell::new(" ");
        cell.set_bg(Color::Rgb(self.r(), self.g(), self.b()));
        cell
    }
}

fn buffer_from_pixels<const F: u32>(
    pixels: &[Pixel<F>],
    size: Size<u32, Physical>,
) -> ratatui::buffer::Buffer {
    let mut buf = ratatui::buffer::Buffer::empty(Rect::new(
        0,
        0,
        size.w.try_into().unwrap(),
        size.h.try_into().unwrap(),
    ));
    pixels
        .iter()
        .zip(buf.content.iter_mut())
        .for_each(|(pixel, cell)| {
            *cell = pixel.into();
        });
    buf
}

fn buffer_from_ptr_len<const F: u32>(
    ptr: *const Pixel<F>,
    len_pixels: usize,
    size: Size<u32, Physical>,
) -> ratatui::buffer::Buffer {
    // SAFETY: TODO
    let pixels: &[Pixel<F>] = unsafe { std::slice::from_raw_parts(ptr, len_pixels) };
    buffer_from_pixels(pixels, size)
}

fn buffer_from_mapping<const F: u32>(
    map: &crate::backend::allocator::dmabuf::DmabufMapping,
    size: Size<u32, Physical>,
) -> ratatui::buffer::Buffer {
    buffer_from_ptr_len::<F>(map.ptr() as *const _, map.length() / 4, size)
}

impl ImportDma for RatatuiRenderer {
    fn import_dmabuf(
        &mut self,
        dmabuf: &crate::backend::allocator::dmabuf::Dmabuf,
        _damage: Option<&[Rectangle<i32, BufferCoord>]>,
    ) -> Result<Self::TextureId, Self::Error> {
        let size = Size::new(dmabuf.width().into(), dmabuf.height().into());
        let map = dmabuf.map_plane(0, DmabufMappingMode::READ).unwrap();
        let buf = match dmabuf.format().code {
            Fourcc::Argb8888 => buffer_from_mapping::<{ Fourcc::Argb8888 as _ }>(&map, size),
            Fourcc::Xrgb8888 => buffer_from_mapping::<{ Fourcc::Xrgb8888 as _ }>(&map, size),
            Fourcc::Rgba8888 => buffer_from_mapping::<{ Fourcc::Rgba8888 as _ }>(&map, size),
            Fourcc::Rgbx8888 => buffer_from_mapping::<{ Fourcc::Rgbx8888 as _ }>(&map, size),
            Fourcc::Abgr8888 => buffer_from_mapping::<{ Fourcc::Abgr8888 as _ }>(&map, size),
            Fourcc::Xbgr8888 => buffer_from_mapping::<{ Fourcc::Xbgr8888 as _ }>(&map, size),
            Fourcc::Bgra8888 => buffer_from_mapping::<{ Fourcc::Bgra8888 as _ }>(&map, size),
            Fourcc::Bgrx8888 => buffer_from_mapping::<{ Fourcc::Bgrx8888 as _ }>(&map, size),
            f => todo!("unsupported format: {f:?}"),
        };
        Ok(RatatuiTexture::from(buf))
    }
}

#[cfg(all(
    feature = "wayland_frontend",
    feature = "backend_egl",
    feature = "use_system_lib"
))]
impl ImportEgl for RatatuiRenderer {
    fn bind_wl_display(
        &mut self,
        display: &wayland_server::DisplayHandle,
    ) -> Result<(), crate::backend::egl::Error> {
        todo!()
    }

    fn unbind_wl_display(&mut self) {
        todo!()
    }

    fn egl_reader(&self) -> Option<&EGLBufferReader> {
        todo!()
    }

    fn import_egl_buffer(
        &mut self,
        buffer: &wayland_server::protocol::wl_buffer::WlBuffer,
        surface: Option<&crate::wayland::compositor::SurfaceData>,
        damage: &[Rectangle<i32, BufferCoord>],
    ) -> Result<Self::TextureId, Self::Error> {
        todo!()
    }
}

/// A widget that displays the compositor scene
#[derive(Debug)]
pub struct CompositorWidget;

/// The state of the `CompositorWidget`
#[derive(Debug)]
pub struct CompositorWidgetState;

/// A texture for the ratatui renderer
#[derive(Debug, Clone)]
pub struct RatatuiTexture {
    buffer: Arc<Mutex<ratatui::buffer::Buffer>>,
}

/// TODO: docs
#[derive(thiserror::Error, Debug)]
pub enum RatatuiError {
    /// TODO: docs
    #[error("Texture width or height {}x{} cannot be represented as u16", .0.0, .0.1)]
    TextureTooBig((i32, i32)),
    /// TODO: docs
    #[error("Texture size {}x{} does not match terminal size {}x{}", actual.width, actual.height, expected.width, expected.height)]
    InvalidTextureSize {
        actual: ratatui::layout::Size,
        expected: ratatui::layout::Size,
    },
    #[error("Unsupported pixel format: {0:?}")]
    UnsupportedWlPixelFormat(wayland_server::protocol::wl_shm::Format),
    #[error("Buffer access error: {0:?}")]
    BufferAccessError(#[from] crate::wayland::shm::BufferAccessError),
}

impl RatatuiTexture {
    fn get_pixel(&self, x: f32, y: f32) -> Color {
        let buf = self.buffer.lock().unwrap();
        let x = x * buf.area.width as f32;
        let y = y * buf.area.height as f32;
        let x = u16::try_from(x.round().clamp(0f32, buf.area.width as f32 - 1f32) as i64).unwrap();
        let y = u16::try_from(y.round().clamp(0f32, buf.area.height as f32 - 1f32) as i64).unwrap();
        buf.cell((x, y)).unwrap().bg
    }
}

impl From<ratatui::buffer::Buffer> for RatatuiTexture {
    fn from(value: ratatui::buffer::Buffer) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(value)),
        }
    }
}

impl Texture for RatatuiTexture {
    fn width(&self) -> u32 {
        self.buffer.lock().unwrap().area.width.into()
    }

    fn height(&self) -> u32 {
        self.buffer.lock().unwrap().area.height.into()
    }

    fn format(&self) -> Option<Fourcc> {
        Some(Fourcc::Xrgb8888)
    }
}

/// A frame for the ratatui renderer
#[derive(Debug)]
pub struct RatatuiFrame<'frame, 'buffer> {
    renderer: &'frame mut RatatuiRenderer,
    framebuffer: &'frame mut <RatatuiRenderer as RendererSuper>::Framebuffer<'buffer>,
}

fn color_to_ratatui(color: Color32F) -> Color {
    Color::Rgb(
        (color.r() * 255.0).round() as u8,
        (color.g() * 255.0).round() as u8,
        (color.b() * 255.0).round() as u8,
    )
}

impl RatatuiFrame<'_, '_> {
    fn fill_rect(&mut self, rect: &Rectangle<i32, Physical>, color: Color) {
        let mut buf = self.framebuffer.buffer.lock().unwrap();

        let x_min = rect.loc.x.clamp(0, buf.area.width as i32);
        let x_max = (rect.loc.x + rect.size.w).clamp(0, buf.area.width as i32);
        let y_min = rect.loc.y.clamp(0, buf.area.height as i32);
        let y_max = (rect.loc.y + rect.size.h).clamp(0, buf.area.height as i32);

        for y in y_min..y_max {
            for x in x_min..x_max {
                // TODO wtf is going on
                let cell = buf.cell_mut((
                    x.try_into().expect("x > u16::MAX"),
                    y.try_into().expect("y > u16::MAX"),
                ));
                if let Some(cell) = cell {
                    cell.set_bg(color);
                } else {
                    todo!(
                        "WTF pos {x}, {y} is out of {}x{} bounds",
                        buf.area.width, buf.area.height
                    );
                }
            }
        }
    }
}

impl Drop for RatatuiFrame<'_, '_> {
    fn drop(&mut self) {
        let _ = self.renderer.terminal.draw(|frame| {
            std::mem::swap(
                &mut frame.buffer_mut().content,
                &mut self.framebuffer.buffer.lock().unwrap().content,
            );
        });
    }
}

impl<'buffer> Frame for RatatuiFrame<'_, 'buffer> {
    type Error = RatatuiError;
    type TextureId = RatatuiTexture;

    fn context_id(&self) -> ContextId<Self::TextureId> {
        ContextId(Arc::new(InnerContextId(0)), PhantomData)
    }

    fn clear(&mut self, color: Color32F, at: &[Rectangle<i32, Physical>]) -> Result<(), Self::Error> {
        let color = color_to_ratatui(color);
        for rect in at {
            self.fill_rect(rect, color);
        }
        Ok(())
    }

    fn draw_solid(
        &mut self,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        color: Color32F,
    ) -> Result<(), Self::Error> {
        let color = color_to_ratatui(color);
        for rect in damage {
            let rect = {
                let loc = rect.loc.constrain(dst);
                let size = rect.size.clamp((0, 0), (dst.size.to_point() - loc).to_size());
                Rectangle::new(loc, size)
            };

            self.fill_rect(&rect, color);
        }

        Ok(())
    }

    fn render_texture_from_to(
        &mut self,
        texture: &Self::TextureId,
        _src: Rectangle<f64, BufferCoord>,
        _dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _src_transform: Transform,
        _alpha: f32,
    ) -> Result<(), Self::Error> {
        // TODO src dst
        let mut buf = self.framebuffer.buffer.lock().unwrap();
        for rect in damage {
            let x_min = rect.loc.x.clamp(0, buf.area.width as i32);
            let x_max = (rect.loc.x + rect.size.w).clamp(0, buf.area.width as i32);
            let y_min = rect.loc.y.clamp(0, buf.area.height as i32);
            let y_max = (rect.loc.y + rect.size.h).clamp(0, buf.area.height as i32);

            for y in y_min..y_max {
                for x in x_min..x_max {
                    let xf = x as f32 / rect.size.w as f32;
                    let yf = y as f32 / rect.size.h as f32;
                    let color = texture.get_pixel(xf, yf);
                    // TODO wtf is going on
                    let cell = buf.cell_mut((u16::try_from(x).unwrap(), u16::try_from(y).unwrap()));
                    if let Some(cell) = cell {
                        cell.set_bg(color);
                    } else {
                        todo!(
                            "WTF pos {x}, {y} is out of {}x{} bounds",
                            buf.area.width, buf.area.height
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn transformation(&self) -> Transform {
        Transform::Normal
    }

    fn wait(&mut self, sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        while let Err(Interrupted) = sync.wait() {}
        Ok(())
    }

    fn finish(self) -> Result<sync::SyncPoint, Self::Error> {
        // TODO
        Ok(sync::SyncPoint::default())
    }
}

impl RendererSuper for RatatuiRenderer {
    type Error = RatatuiError;
    type TextureId = RatatuiTexture;
    type Framebuffer<'buffer> = RatatuiTexture;
    type Frame<'frame, 'buffer>
        = RatatuiFrame<'frame, 'buffer>
    where
        'buffer: 'frame,
        Self: 'frame;
}

impl Renderer for RatatuiRenderer {
    fn context_id(&self) -> ContextId<Self::TextureId> {
        ContextId(Arc::new(InnerContextId(0)), PhantomData)
    }

    fn downscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        // TODO
        Ok(())
    }

    fn upscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        // TODO
        Ok(())
    }

    fn set_debug_flags(&mut self, _flags: DebugFlags) {}

    fn debug_flags(&self) -> DebugFlags {
        // TODO
        DebugFlags::empty()
    }

    fn render<'frame, 'buffer>(
        &'frame mut self,
        framebuffer: &'frame mut Self::Framebuffer<'buffer>,
        _output_size: Size<i32, Physical>,
        _dst_transform: Transform,
    ) -> Result<Self::Frame<'frame, 'buffer>, Self::Error>
    where
        'buffer: 'frame,
    {
        Ok(RatatuiFrame {
            renderer: self,
            framebuffer,
        })
    }

    fn wait(&mut self, _sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        todo!()
    }
}

impl crate::backend::renderer::ImportMem for RatatuiRenderer {
    fn import_memory(
        &mut self,
        _data: &[u8],
        _format: Fourcc,
        size: Size<i32, BufferCoord>,
        _flipped: bool,
    ) -> Result<Self::TextureId, Self::Error> {
        let (Ok(w), Ok(h)) = (u16::try_from(size.w), u16::try_from(size.h)) else {
            return Err(RatatuiError::TextureTooBig((size.w, size.h)));
        };

        let buf = ratatui::buffer::Buffer::empty(Rect::new(0, 0, w, h));

        Ok(RatatuiTexture {
            buffer: Arc::new(Mutex::new(buf)),
        })
    }

    fn update_memory(
        &mut self,
        _texture: &Self::TextureId,
        _data: &[u8],
        _region: Rectangle<i32, BufferCoord>,
    ) -> Result<(), Self::Error> {
        // TODO
        Ok(())
    }

    fn mem_formats(&self) -> Box<dyn Iterator<Item = Fourcc>> {
        const SUPPORTED_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Xrgb8888];
        Box::new(SUPPORTED_FORMATS.iter().cloned())
    }
}
