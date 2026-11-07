use genie_render::scene::{Align, Font, Paint, Primitive};
use iced::advanced::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _, Text};
use iced::advanced::widget::{Tree, Widget};
use iced::{
    Color, Element, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment, mouse,
};

use crate::font;
use crate::theme::{DataColors, Tokens, data, tokens};

struct Scene {
    primitives: Vec<Primitive>,
    height: f32,
}

pub fn view<'a, Message: 'a>(primitives: Vec<Primitive>, height: f32) -> Element<'a, Message> {
    Element::new(Scene { primitives, height })
}

pub fn color(paint: Paint, tokens: &Tokens, data: &DataColors) -> Color {
    match paint {
        Paint::Base(base) => data.bases[base as usize],
        Paint::OnBase(base) => data.on_bases[base as usize],
        Paint::Muted => tokens.muted,
        Paint::Line => tokens.line,
        Paint::Accent => tokens.accent,
    }
}

fn typeface(font: Font) -> iced::Font {
    match font {
        Font::Ui => font::UI,
        Font::Mono => font::MONO,
        Font::MonoStrong => font::MONO_STRONG,
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Scene {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.height))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Length::Fill, self.height)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let (tokens, data) = (tokens(theme), data(theme));
        renderer.with_layer(bounds, |renderer| {
            for primitive in &self.primitives {
                match primitive {
                    Primitive::Rect {
                        x,
                        y,
                        width,
                        height,
                        paint,
                    } => renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x: bounds.x + x,
                                y: bounds.y + y,
                                width: *width,
                                height: *height,
                            },
                            ..Quad::default()
                        },
                        color(*paint, tokens, data),
                    ),
                    Primitive::Text {
                        x,
                        y,
                        content,
                        size,
                        paint,
                        font,
                        align,
                    } => renderer.fill_text(
                        Text {
                            content: content.to_string(),
                            bounds: bounds.size(),
                            size: Pixels(*size),
                            line_height: text::LineHeight::Relative(1.0),
                            font: typeface(*font),
                            align_x: match align {
                                Align::Start => text::Alignment::Left,
                                Align::Center => text::Alignment::Center,
                            },
                            align_y: alignment::Vertical::Center,
                            shaping: text::Shaping::Basic,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(bounds.x + x, bounds.y + y),
                        color(*paint, tokens, data),
                        bounds,
                    ),
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use genie_render::scene::{Base, Paint};
    use iced::widget::container;
    use iced_test::{selector, simulator};

    use super::{color, view};
    use crate::theme::{DARK, DARK_DATA, LIGHT, LIGHT_DATA};

    #[test]
    fn base_paints_use_data_colors() {
        assert_eq!(
            color(Paint::Base(Base::G), &LIGHT, &LIGHT_DATA),
            LIGHT_DATA.bases[2]
        );
        assert_eq!(
            color(Paint::OnBase(Base::T), &DARK, &DARK_DATA),
            DARK_DATA.on_bases[3]
        );
    }

    #[test]
    fn chrome_paints_use_theme_tokens() {
        assert_eq!(color(Paint::Muted, &LIGHT, &LIGHT_DATA), LIGHT.muted);
        assert_eq!(color(Paint::Line, &DARK, &DARK_DATA), DARK.line);
        assert_eq!(color(Paint::Accent, &DARK, &DARK_DATA), DARK.accent);
    }

    #[test]
    fn keeps_its_height() {
        let mut ui = simulator(container(view::<()>(Vec::new(), 20.0)).id("scene"));
        let scene = ui.find(selector::id("scene")).unwrap();
        assert!((scene.bounds().height - 20.0).abs() < f32::EPSILON);
    }
}
