#![cfg_attr(docsrs, doc(cfg(feature = "ratatui_backend")))]

use crate::backend::renderer::{
    Frame, Renderer, Texture, RendererSuper, TextureFilter, DebugFlags, sync, ContextId,
    Color32F,
};
use std::any::Any;
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
pub struct RatatuiTexture;

impl Texture for RatatuiTexture {
    fn width(&self) -> u32 {
        todo!()
    }

    fn height(&self) -> u32 {
        todo!()
    }

    fn format(&self) -> Option<crate::backend::allocator::Fourcc> {
        todo!()
    }
}

/// A frame for the ratatui renderer
#[derive(Debug)]
pub struct RatatuiFrame<'frame, 'buffer> {
    _frame: PhantomData<&'frame ()>,
    _buffer: PhantomData<&'buffer ()>
}

impl Frame for RatatuiFrame<'_, '_> {
    type Error = std::convert::Infallible;
    type TextureId = RatatuiTexture;

    fn context_id(&self) -> ContextId<Self::TextureId> {
        todo!()
    }

    fn clear(&mut self, _color: Color32F, _at: &[Rectangle<i32, Physical>]) -> Result<(), Self::Error> {
        todo!()
    }

    fn draw_solid(
        &mut self,
        _dst: Rectangle<i32, Physical>,
        _damage: &[Rectangle<i32, Physical>],
        _color: Color32F,
    ) -> Result<(), Self::Error> {
        todo!()
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
        todo!()
    }

    fn transformation(&self) -> Transform {
        todo!()
    }

    fn wait(&mut self, _sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        todo!()
    }

    fn finish(self) -> Result<sync::SyncPoint, Self::Error> {
        todo!()
    }
}

impl RendererSuper for RatatuiRenderer {
    type Error = std::convert::Infallible;
    type TextureId = RatatuiTexture;
    type Framebuffer<'buffer> = ratatui::buffer::Buffer;
    type Frame<'frame, 'buffer> = RatatuiFrame<'frame, 'buffer> where 'buffer: 'frame, Self: 'frame;
}

impl Renderer for RatatuiRenderer {
    fn context_id(&self) -> ContextId<Self::TextureId> {
        todo!()
    }

    fn downscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        todo!()
    }

    fn upscale_filter(&mut self, _filter: TextureFilter) -> Result<(), Self::Error> {
        todo!()
    }

    fn set_debug_flags(&mut self, _flags: DebugFlags) {}

    fn debug_flags(&self) -> DebugFlags {
        todo!()
    }

    fn render<'frame, 'buffer>(
        &'frame mut self,
        framebuffer: &'frame mut Self::Framebuffer<'buffer>,
        _output_size: Size<i32, Physical>,
        _dst_transform: Transform,
    ) -> Result<Self::Frame<'frame, 'buffer>, Self::Error> where 'buffer: 'frame {
        todo!()
    }

    fn wait(&mut self, _sync: &sync::SyncPoint) -> Result<(), Self::Error> {
        todo!()
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
        todo!()
    }

    fn update_memory(
        &mut self,
        _texture: &Self::TextureId,
        _data: &[u8],
        _region: Rectangle<i32, BufferCoord>,
    ) -> Result<(), Self::Error> {
        todo!()
    }

    fn mem_formats(
        &self,
    ) -> Box<dyn Iterator<Item = crate::backend::allocator::Fourcc>> {
        todo!()
    }
}