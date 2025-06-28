'''use crate::backend::renderer::{Frame, Renderer, Texture};
use crate::utils::{Buffer, Physical, Rectangle, Size};
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use std::borrow::Cow;
use std::io;

const TEST_TEXTURE_ID: usize = 10;

#[derive(Debug, Clone)]
pub struct RatatuiTexture {
    id: usize,
    size: Size<i32, Buffer>,
}

impl PartialEq for RatatuiTexture {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Texture for RatatuiTexture {
    fn width(&self) -> u32 {
        self.size.w as u32
    }
    fn height(&self) -> u32 {
        self.size.h as u32
    }
}

#[derive(Debug)]
pub struct RatatuiFrame<'a, B: Backend> {
    terminal: &'a mut Terminal<B>,
}

impl<'a, B: Backend> Frame for RatatuiFrame<'a, B> {
    type Error = io::Error;
    type TextureId = RatatuiTexture;

    fn id(&self) -> usize {
        0
    }

    fn clear(&mut self, _color: [f32; 4]) -> Result<(), Self::Error> {
        self.terminal.clear()
    }

    fn render_texture_at(
        &mut self,
        texture: &Self::TextureId,
        pos: Physical<i32>,
        _scale: i32,
        _alpha: f32,
    ) -> Result<(), Self::Error> {
        self.terminal.draw(|f| {
            let size = f.size();
            let title = format!("Texture #{}", texture.id);
            let block = Block::default().title(title).borders(Borders::ALL);
            let para = Paragraph::new(Cow::Owned(format!("{:?}", texture.size)));
            let rect = Rect::new(pos.x as u16, pos.y as u16, texture.width() as u16, texture.height() as u16);
            f.render_widget(para.block(block), rect);
        })?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct RatatuiRenderer<B: Backend> {
    terminal: Terminal<B>,
}

impl<B: Backend> RatatuiRenderer<B> {
    pub fn new(terminal: Terminal<B>) -> Self {
        RatatuiRenderer { terminal }
    }
}

impl<B: Backend> Renderer for RatatuiRenderer<B> {
    type Error = io::Error;
    type TextureId = RatatuiTexture;
    type Frame<'a> = RatatuiFrame<'a, B>;

    fn id(&self) -> usize {
        0
    }

    fn render<'a>(
        &'a mut self,
        _size: Physical<i32>,
        _transform: crate::utils::Transform,
    ) -> Result<Self::Frame<'a>, Self::Error> {
        Ok(RatatuiFrame {
            terminal: &mut self.terminal,
        })
    }
}
''