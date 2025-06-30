#![cfg_attr(docsrs, doc(cfg(feature = "ratatui_backend")))]

use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::backend::allocator::Fourcc;
use crate::backend::renderer::sync::Interrupted;
use crate::backend::renderer::{
    sync, Color32F, ContextId, DebugFlags, Frame, InnerContextId, Renderer, RendererSuper, Texture, TextureFilter
};
use crate::utils::{Buffer as BufferCoord, Physical, Rectangle, Size, Transform};

/// A renderer for the ratatui backend
#[derive(Debug)]
pub struct RatatuiRenderer;

/// A widget that displays the compositor scene
#[derive(Debug)]
pub struct CompositorWidget;

/// The state of the `CompositorWidget`
#[derive(Debug)]
pub struct CompositorWidgetState;

/// A texture for the ratatui renderer
#[derive(Debug)]
pub struct RatatuiTexture {
    buffer: Arc<Mutex<Buffer>>,
}

#[derive(thiserror::Error, Debug)]
pub enum RatatuiError {
    #[error("Texture width or height {}x{} cannot be represented as u16", .0.0, .0.1)]
    TextureTooBig((i32, i32)),
}

impl RatatuiTexture {
    fn get_pixel(&self, x: f32, y: f32) -> Color {
        let buf = self.buffer.lock().unwrap();
        let x = x * buf.area.width as f32;
        let y = y * buf.area.height as f32;
        let x = x.round().clamp(0f32, buf.area.width as f32 - 1f32) as u16;
        let y = y.round().clamp(0f32, buf.area.height as f32 - 1f32) as u16;
        buf.get(x, y).bg
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
    renderer: &'frame RatatuiRenderer,
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

        for y in rect.loc.y..rect.loc.y + rect.size.h {
            for x in rect.loc.x..rect.loc.x + rect.size.w {
                let cell = buf.get_mut(
                    x.try_into().expect("x > u16::MAX"),
                    y.try_into().expect("y > u16::MAX"),
                );
                cell.set_bg(color);
            }
        }
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
            for y in rect.loc.y..rect.loc.y + rect.size.h {
                for x in rect.loc.x..rect.loc.x + rect.size.w {
                    let xf = x as f32 / buf.area.width as f32;
                    let yf = y as f32 / buf.area.height as f32;
                    let color = texture.get_pixel(xf, yf);
                    buf.get_mut(x as u16, y as u16).set_bg(color);
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
        Ok(RatatuiFrame { renderer: self, framebuffer })
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

        let buf = Buffer::empty(Rect::new(0, 0, w, h));

        Ok(RatatuiTexture {
            buffer: Arc::new(Mutex::new(buf))
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
        const SUPPORTED_FORMATS: [Fourcc; 2] = [
            Fourcc::Argb8888,
            Fourcc::Xrgb8888,
        ];
        Box::new(SUPPORTED_FORMATS.iter().cloned())
    }
}
