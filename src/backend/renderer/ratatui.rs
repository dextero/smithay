#![cfg_attr(docsrs, doc(cfg(feature = "ratatui_backend")))]

use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use crossterm::ExecutableCommand;
use indexmap::Equivalent;
use ratatui::layout::Rect;
use ratatui::prelude::CrosstermBackend;
use ratatui::style::Color;
use ratatui::Terminal;

use crate::backend::allocator::dmabuf::DmabufMappingMode;
use crate::backend::allocator::{Buffer, Fourcc};
use crate::backend::renderer::sync::Interrupted;
use crate::backend::renderer::{
    sync, Color32F, ContextId, DebugFlags, Frame, ImportDma, ImportDmaWl, ImportMemWl, InnerContextId,
    Renderer, RendererSuper, Texture, TextureFilter,
};
use crate::utils::{Buffer as BufferCoord, Physical, Point, Rectangle, Size, Transform};

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
        std::io::stdout()
            .execute(crossterm::event::EnableMouseCapture)
            .unwrap()
            .execute(crossterm::event::PushKeyboardEnhancementFlags(
                crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                    | crossterm::event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES,
            ))
            .unwrap();
        Self { terminal }
    }

    /// TODO
    pub fn terminal_size(&self) -> ratatui::layout::Size {
        self.terminal.size().unwrap()
    }

    /// Return the window size, in cells
    pub fn window_size(&self) -> Size<i32, Physical> {
        let size = self.terminal_size();
        Size::new(size.width.into(), i32::from(size.height) * 2)
    }

    /// TODO
    pub fn swap_buffers(&mut self, mut fb: RatatuiFramebuffer) -> Result<RatatuiFramebuffer, RatatuiError> {
        let expected_size = self.terminal_size();
        let actual_size = fb.buffer.area.as_size();
        if expected_size != actual_size {
            // window resized
            return Ok(self.new_framebuffer());
        }
        std::mem::swap(self.terminal.current_buffer_mut(), &mut fb.buffer);
        self.terminal.flush()?;
        Ok(fb)
    }

    /// TODO: docs
    pub fn new_framebuffer(&self) -> RatatuiFramebuffer {
        let size = self.terminal_size();
        let buffer = ratatui::buffer::Buffer::empty(Rect::new(0, 0, size.width, size.height));
        RatatuiFramebuffer { buffer }
    }
}

impl Drop for RatatuiRenderer {
    fn drop(&mut self) {
        let _ = std::io::stdout().execute(crossterm::event::DisableMouseCapture);
        ratatui::restore();
    }
}

/// TODO: docs
#[derive(Debug)]
pub struct RatatuiFramebuffer {
    buffer: ratatui::buffer::Buffer,
}

impl RatatuiFramebuffer {
    fn is_compatible_with(&self, renderer: &RatatuiRenderer) -> bool {
        let expected_size = renderer.terminal_size();
        let actual_size = self.buffer.area.as_size();
        expected_size == actual_size
    }
}

impl Texture for RatatuiFramebuffer {
    fn width(&self) -> u32 {
        u32::from(self.buffer.area().width)
    }

    fn height(&self) -> u32 {
        u32::from(self.buffer.area().height)
    }

    fn format(&self) -> Option<Fourcc> {
        Some(Fourcc::Argb8888)
    }
}

fn pixels_into_argb8888<const F: u32>(ptr: *const u8, size: usize) -> Vec<PixelArgb8888> {
    assert!(size % 4 == 0);
    let slice: &[Pixel<F>] = unsafe { std::slice::from_raw_parts(ptr as *const Pixel<F>, size / 4) };
    slice.iter().map(IntoArgb8888::into_argb8888).collect()
}

impl ImportMemWl for RatatuiRenderer {
    fn import_shm_buffer<'buf>(
        &mut self,
        buffer: &'buf wayland_server::protocol::wl_buffer::WlBuffer,
        _surface: Option<&crate::wayland::compositor::SurfaceData>,
        _damage: &[Rectangle<i32, BufferCoord>],
    ) -> Result<Self::TextureId, Self::Error> {
        with_buffer_contents(buffer, |ptr, len, data| -> Result<Self::TextureId, Self::Error> {
            let size = Size::new(data.width.try_into().unwrap(), data.height.try_into().unwrap());
            let fourcc = shm_format_to_fourcc(data.format)
                .ok_or(RatatuiError::UnsupportedWlPixelFormat(data.format))?;

            let pixels = match fourcc {
                Fourcc::Argb8888 => pixels_into_argb8888::<{ Fourcc::Argb8888 as u32 }>(ptr, len),
                Fourcc::Xrgb8888 => pixels_into_argb8888::<{ Fourcc::Xrgb8888 as u32 }>(ptr, len),
                Fourcc::Rgba8888 => pixels_into_argb8888::<{ Fourcc::Rgba8888 as u32 }>(ptr, len),
                Fourcc::Rgbx8888 => pixels_into_argb8888::<{ Fourcc::Rgbx8888 as u32 }>(ptr, len),
                Fourcc::Abgr8888 => pixels_into_argb8888::<{ Fourcc::Abgr8888 as u32 }>(ptr, len),
                Fourcc::Xbgr8888 => pixels_into_argb8888::<{ Fourcc::Xbgr8888 as u32 }>(ptr, len),
                Fourcc::Bgra8888 => pixels_into_argb8888::<{ Fourcc::Bgra8888 as u32 }>(ptr, len),
                Fourcc::Bgrx8888 => pixels_into_argb8888::<{ Fourcc::Bgrx8888 as u32 }>(ptr, len),
                f => todo!("unsupported format: {f:?}"),
            };
            let tex = RatatuiTexture { pixels, size };
            Ok(tex.into())
        })?
    }
}

impl ImportDmaWl for RatatuiRenderer {}

trait Blend {
    fn blend_with<const F: u32>(&mut self, fg_pix: Option<Pixel<F>>, bg_pix: Option<Pixel<F>>, alpha: f32);
}

impl Blend for ratatui::buffer::Cell {
    fn blend_with<const F: u32>(&mut self, fg_pix: Option<Pixel<F>>, bg_pix: Option<Pixel<F>>, alpha: f32) {
        assert!(0f32 <= alpha && alpha <= 1f32);

        fn blend(bg: (u8, u8, u8), fg: (u8, u8, u8), alpha: f32) -> Color {
            let one_minus_alpha = 1f32 - alpha;
            let r = (fg.0 as f32 * alpha + bg.0 as f32 * one_minus_alpha) as u8;
            let g = (fg.1 as f32 * alpha + bg.1 as f32 * one_minus_alpha) as u8;
            let b = (fg.2 as f32 * alpha + bg.2 as f32 * one_minus_alpha) as u8;
            Color::Rgb(r, g, b)
        }

        match (self.fg, fg_pix) {
            (Color::Rgb(r, g, b), Some(pix)) => {
                let alpha = pix.a() as f32 / 255f32 * alpha;
                self.fg = blend((r, g, b), (pix.r(), pix.g(), pix.b()), alpha);
            }
            (_, Some(pix)) => self.fg = pix.into(),
            (_, None) => {}
        }

        match (self.bg, bg_pix) {
            (Color::Rgb(r, g, b), Some(pix)) => {
                let alpha = pix.a() as f32 / 255f32 * alpha;
                self.bg = blend((r, g, b), (pix.r(), pix.g(), pix.b()), alpha);
            }
            (_, Some(pix)) => self.bg = pix.into(),
            (_, None) => {}
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
struct Pixel<const F: u32>(u32);

type PixelArgb8888 = Pixel<{ Fourcc::Argb8888 as u32 }>;

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

    fn a(&self) -> u8 {
        match Fourcc::try_from(F) {
            Ok(Fourcc::Argb8888) => (self.0 >> 24) as u8,
            Ok(Fourcc::Xrgb8888) => u8::MAX,
            Ok(Fourcc::Rgba8888) => (self.0 >> 0) as u8,
            Ok(Fourcc::Rgbx8888) => u8::MAX,
            Ok(Fourcc::Abgr8888) => (self.0 >> 24) as u8,
            Ok(Fourcc::Xbgr8888) => u8::MAX,
            Ok(Fourcc::Bgra8888) => (self.0 >> 0) as u8,
            Ok(Fourcc::Bgrx8888) => u8::MAX,
            Ok(f) => todo!("unsupported format: {f:?}"),
            Err(e) => todo!("invalid format: {e:?}"),
        }
    }
}

trait IntoArgb8888 {
    fn into_argb8888(self) -> PixelArgb8888;
}

impl<const F: u32> IntoArgb8888 for &Pixel<F> {
    fn into_argb8888(self) -> PixelArgb8888 {
        (*self).into_argb8888()
    }
}

impl<const F: u32> IntoArgb8888 for Pixel<F> {
    fn into_argb8888(self) -> PixelArgb8888 {
        Pixel::<{ Fourcc::Argb8888 as u32 }>(
            u32::from(self.a()) << 24
                | u32::from(self.r()) << 16
                | u32::from(self.g()) << 8
                | u32::from(self.b()) << 0,
        )
    }
}

impl<const F: u32> Into<Color> for Pixel<F> {
    fn into(self) -> Color {
        Color::Rgb(self.r(), self.g(), self.b())
    }
}

impl ImportDma for RatatuiRenderer {
    fn import_dmabuf(
        &mut self,
        dmabuf: &crate::backend::allocator::dmabuf::Dmabuf,
        _damage: Option<&[Rectangle<i32, BufferCoord>]>,
    ) -> Result<Self::TextureId, Self::Error> {
        let size = Size::new(dmabuf.width().into(), dmabuf.height().into());
        let map = dmabuf.map_plane(0, DmabufMappingMode::READ).unwrap();
        let ptr = map.ptr() as *const u8;
        let len = map.length();
        let pixels = match dmabuf.format().code {
            Fourcc::Argb8888 => pixels_into_argb8888::<{ Fourcc::Argb8888 as u32 }>(ptr, len),
            Fourcc::Xrgb8888 => pixels_into_argb8888::<{ Fourcc::Xrgb8888 as u32 }>(ptr, len),
            Fourcc::Rgba8888 => pixels_into_argb8888::<{ Fourcc::Rgba8888 as u32 }>(ptr, len),
            Fourcc::Rgbx8888 => pixels_into_argb8888::<{ Fourcc::Rgbx8888 as u32 }>(ptr, len),
            Fourcc::Abgr8888 => pixels_into_argb8888::<{ Fourcc::Abgr8888 as u32 }>(ptr, len),
            Fourcc::Xbgr8888 => pixels_into_argb8888::<{ Fourcc::Xbgr8888 as u32 }>(ptr, len),
            Fourcc::Bgra8888 => pixels_into_argb8888::<{ Fourcc::Bgra8888 as u32 }>(ptr, len),
            Fourcc::Bgrx8888 => pixels_into_argb8888::<{ Fourcc::Bgrx8888 as u32 }>(ptr, len),
            f => todo!("unsupported format: {f:?}"),
        };
        let tex = RatatuiTexture { pixels, size };
        Ok(tex.into())
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
        _display: &wayland_server::DisplayHandle,
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
        _buffer: &wayland_server::protocol::wl_buffer::WlBuffer,
        _surface: Option<&crate::wayland::compositor::SurfaceData>,
        _damage: &[Rectangle<i32, BufferCoord>],
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
    pixels: Vec<PixelArgb8888>,
    size: Size<u32, Physical>,
}

/// TODO: doc
#[derive(Clone, Debug)]
pub struct RatatuiTextureHandle(Arc<Mutex<RatatuiTexture>>);

impl From<RatatuiTexture> for RatatuiTextureHandle {
    fn from(value: RatatuiTexture) -> Self {
        Self(Arc::new(Mutex::new(value)))
    }
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
        /// TODO: docs
        actual: ratatui::layout::Size,
        /// TODO: docs
        expected: ratatui::layout::Size,
    },
    /// TODO: docs
    #[error("Unsupported pixel format: {0:?}")]
    UnsupportedWlPixelFormat(wayland_server::protocol::wl_shm::Format),
    /// TODO: docs
    #[error("Buffer access error: {0:?}")]
    BufferAccessError(#[from] crate::wayland::shm::BufferAccessError),
    /// TODO: docs
    #[error("IO error: {0:?}")]
    IoError(#[from] std::io::Error),
}

impl RatatuiTexture {
    fn get_pixel(&self, p: Point<f64, BufferCoord>) -> PixelArgb8888 {
        let x = u16::try_from(p.x.round().clamp(0f64, self.size.w as f64 - 1f64) as i64).unwrap();
        let y = u16::try_from(p.y.round().clamp(0f64, self.size.h as f64 - 1f64) as i64).unwrap();
        let idx = y as usize * self.size.w as usize + x as usize;
        *self.pixels.get(idx).unwrap()
    }
}

impl Texture for RatatuiTextureHandle {
    fn width(&self) -> u32 {
        u32::try_from(self.0.lock().unwrap().size.w).unwrap()
    }

    fn height(&self) -> u32 {
        u32::try_from(self.0.lock().unwrap().size.h).unwrap()
    }

    fn format(&self) -> Option<Fourcc> {
        Some(Fourcc::Argb8888)
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

impl<'frame> RatatuiFrame<'frame, '_> {
    fn new(renderer: &'frame mut RatatuiRenderer, framebuffer: &'frame mut RatatuiFramebuffer) -> Self {
        if !framebuffer.is_compatible_with(renderer) {
            tracing::warn!(
                "window resized? fb {:?}, terminal {:?}; creating new framebuffer",
                framebuffer.buffer.area.as_size(),
                renderer.terminal_size()
            );
            *framebuffer = renderer.new_framebuffer();
        }

        Self {
            renderer,
            framebuffer,
        }
    }

    fn fill_rect(&mut self, rect: &Rectangle<i32, Physical>, color: Color) {
        let buf = &mut self.framebuffer.buffer;

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
                &mut self.framebuffer.buffer.content,
            );
        });
    }
}

impl<'buffer> Frame for RatatuiFrame<'_, 'buffer> {
    type Error = RatatuiError;
    type TextureId = RatatuiTextureHandle;

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
        _damage: &[Rectangle<i32, Physical>],
        color: Color32F,
    ) -> Result<(), Self::Error> {
        let color = color_to_ratatui(color);
        self.fill_rect(&dst, color);
        //for rect in damage {
        //    let rect = {
        //        let loc = rect.loc.constrain(dst);
        //        let size = rect.size.clamp((0, 0), (dst.size.to_point() - loc).to_size());
        //        Rectangle::new(loc, size)
        //    };

        //    self.fill_rect(&rect, color);
        //}

        Ok(())
    }

    fn render_texture_from_to(
        &mut self,
        texture: &Self::TextureId,
        src: Rectangle<f64, BufferCoord>,
        _dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _src_transform: Transform,
        alpha: f32,
    ) -> Result<(), Self::Error> {
        // TODO src dst
        let texture = texture.0.lock().unwrap();
        let buf = &mut self.framebuffer.buffer;

        for rect in damage {
            let x_min = rect.loc.x.clamp(0, buf.area.width as i32);
            let x_max = (rect.loc.x + rect.size.w).clamp(0, buf.area.width as i32);
            let y_min = rect.loc.y.clamp(0, buf.area.height as i32 * 2);
            let y_max = (rect.loc.y + rect.size.h).clamp(0, buf.area.height as i32 * 2);

            let row_min = y_min / 2;
            let row_max = (y_max + 1) / 2;

            if y_min % 2 != 0 {
                // first row
                let y = y_min;
                for x in x_min..x_max {
                    let pixel =
                        texture.get_pixel(src.loc + Point::<f64, BufferCoord>::new(x as f64, y as f64));
                    let cell = buf.cell_mut((u16::try_from(x).unwrap(), u16::try_from(row_min).unwrap()));
                    if let Some(cell) = cell {
                        cell.set_char('\u{2584}');
                        cell.blend_with(Some(pixel), None, alpha);
                    }
                }
            }

            for row in row_min..row_max {
                // middle
                let y_top = row * 2;
                let y_bottom = y_top + 1;
                for x in x_min..x_max {
                    let pixel_top =
                        texture.get_pixel(src.loc + Point::<f64, BufferCoord>::new(x as f64, y_top as f64));
                    let pixel_bottom = texture
                        .get_pixel(src.loc + Point::<f64, BufferCoord>::new(x as f64, y_bottom as f64));
                    let cell = buf.cell_mut((u16::try_from(x).unwrap(), u16::try_from(row).unwrap()));
                    if let Some(cell) = cell {
                        cell.set_char('\u{2584}');
                        cell.blend_with(Some(pixel_bottom), Some(pixel_top), alpha);
                    }
                }
            }

            if y_max % 2 == 0 {
                // last row
                let y = y_max - 1;
                for x in x_min..x_max {
                    let pixel =
                        texture.get_pixel(src.loc + Point::<f64, BufferCoord>::new(x as f64, y as f64));
                    let cell = buf.cell_mut((u16::try_from(x).unwrap(), u16::try_from(y / 2).unwrap()));
                    if let Some(cell) = cell {
                        cell.set_char('\u{2584}');
                        cell.blend_with(None, Some(pixel), alpha);
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
    type TextureId = RatatuiTextureHandle;
    type Framebuffer<'buffer> = RatatuiFramebuffer;
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
        Ok(RatatuiFrame::new(self, framebuffer))
    }

    fn wait(&mut self, _sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        todo!()
    }
}

impl crate::backend::renderer::ImportMem for RatatuiRenderer {
    fn import_memory(
        &mut self,
        data: &[u8],
        format: Fourcc,
        size: Size<i32, BufferCoord>,
        _flipped: bool,
    ) -> Result<Self::TextureId, Self::Error> {
        let (Ok(w), Ok(h)) = (u16::try_from(size.w), u16::try_from(size.h)) else {
            return Err(RatatuiError::TextureTooBig((size.w, size.h)));
        };
        let size = Size::new(w.into(), h.into());

        let ptr = data.as_ptr();
        let len = data.len();
        let pixels = match format {
            Fourcc::Argb8888 => pixels_into_argb8888::<{ Fourcc::Argb8888 as u32 }>(ptr, len),
            Fourcc::Xrgb8888 => pixels_into_argb8888::<{ Fourcc::Xrgb8888 as u32 }>(ptr, len),
            Fourcc::Rgba8888 => pixels_into_argb8888::<{ Fourcc::Rgba8888 as u32 }>(ptr, len),
            Fourcc::Rgbx8888 => pixels_into_argb8888::<{ Fourcc::Rgbx8888 as u32 }>(ptr, len),
            Fourcc::Abgr8888 => pixels_into_argb8888::<{ Fourcc::Abgr8888 as u32 }>(ptr, len),
            Fourcc::Xbgr8888 => pixels_into_argb8888::<{ Fourcc::Xbgr8888 as u32 }>(ptr, len),
            Fourcc::Bgra8888 => pixels_into_argb8888::<{ Fourcc::Bgra8888 as u32 }>(ptr, len),
            Fourcc::Bgrx8888 => pixels_into_argb8888::<{ Fourcc::Bgrx8888 as u32 }>(ptr, len),
            f => todo!("unsupported format: {f:?}"),
        };

        let tex = RatatuiTexture { pixels, size };
        Ok(tex.into())
    }

    fn update_memory(
        &mut self,
        _texture: &Self::TextureId,
        _data: &[u8],
        _region: Rectangle<i32, BufferCoord>,
    ) -> Result<(), Self::Error> {
        // TODO
        todo!("ImportMem::update_memory")
    }

    fn mem_formats(&self) -> Box<dyn Iterator<Item = Fourcc>> {
        const SUPPORTED_FORMATS: [Fourcc; 2] = [Fourcc::Argb8888, Fourcc::Xrgb8888];
        Box::new(SUPPORTED_FORMATS.iter().cloned())
    }
}
