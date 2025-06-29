#![cfg_attr(docsrs, doc(cfg(feature = "ratatui_backend")))]

use crate::backend::renderer::{
    element::RenderElement,
    sync,
    utils::{draw_render_elements, on_commit_buffer_handler},
    ContextId, DebugFlags, Frame, Renderer, RendererSuper, Texture, TextureFilter,
};
use crate::utils::{Buffer as BufferCoord, Physical, Point, Rectangle, Scale, Size, Transform};
use std::{any::Any, borrow::Cow, marker::PhantomData};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RatatuiTexture {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

impl Texture for RatatuiTexture {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn format(&self) -> Option<crate::backend::allocator::Fourcc> {
        Some(crate::backend::allocator::Fourcc::Argb8888)
    }
}

impl Texture for ratatui::buffer::Buffer {
    fn width(&self) -> u32 {
        self.area.width as u32
    }

    fn height(&self) -> u32 {
        self.area.height as u32
    }

    fn format(&self) -> Option<crate::backend::allocator::Fourcc> {
        None
    }
}

/// A frame for the ratatui renderer
pub struct RatatuiFrame<'frame, 'buffer> {
    renderer: &'frame mut RatatuiRenderer,
    buffer: &'frame mut ratatui::buffer::Buffer,
    _phantom: PhantomData<&'buffer ()>,
}

impl Frame for RatatuiFrame<'_, '_> {
    type Error = std::convert::Infallible;
    type TextureId = RatatuiTexture;

    fn context_id(&self) -> ContextId<Self::TextureId> {
        ContextId::new()
    }

    fn clear(
        &mut self,
        _color: crate::backend::renderer::Color32F,
        _at: &[Rectangle<i32, Physical>],
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn draw_solid(
        &mut self,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
        _color: crate::backend::renderer::Color32F,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn render_texture_from_to(
        &mut self,
        _texture: &Self::TextureId,
        _src: Rectangle<f64, BufferCoord>,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
        _opaque_regions: &[Rectangle<i32, Physical>],
        _src_transform: Transform,
        _alpha: f32,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn transformation(&self) -> Transform {
        Transform::Normal
    }

    fn wait(&mut self, _sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        Ok(())
    }

    fn finish(self) -> Result<sync::SyncPoint, Self::Error> {
        Ok(sync::SyncPoint::signaled())
    }
}

impl RendererSuper for RatatuiRenderer {
    type Error = std::convert::Infallible;
    type TextureId = RatatuiTexture;
    type Framebuffer<'buffer> = ratatui::buffer::Buffer;
    type Frame<'frame, 'buffer>
        = RatatuiFrame<'frame, 'buffer>
    where
        'buffer: 'frame;
}

impl Renderer for RatatuiRenderer {
    fn context_id(&self) -> ContextId<Self::TextureId> {
        ContextId::new()
    }

    fn downscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        Ok(())
    }

    fn upscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        Ok(())
    }

    fn set_debug_flags(&mut self, _flags: DebugFlags) {}

    fn debug_flags(&self) -> DebugFlags {
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
            buffer: framebuffer,
            _phantom: PhantomData,
        })
    }

    fn wait(&mut self, _sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl crate::backend::renderer::ImportMem for RatatuiRenderer {
    fn import_memory(
        &mut self,
        data: &[u8],
        _format: crate::backend::allocator::Fourcc,
        size: Size<i32, BufferCoord>,
        _flipped: bool,
    ) -> Result<Self::TextureId, Self::Error> {
        Ok(RatatuiTexture {
            pixels: data.iter().map(|x| *x as u32).collect(),
            width: size.w as u32,
            height: size.h as u32,
        })
    }

    fn update_memory(
        &mut self,
        _texture: &Self::TextureId,
        _data: &[u8],
        _region: Rectangle<i32, BufferCoord>,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn mem_formats(&self) -> Box<dyn Iterator<Item = crate::backend::allocator::Fourcc>> {
        Box::new(vec![crate::backend::allocator::Fourcc::Argb8888].into_iter())
    }
}
