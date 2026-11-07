use genie_render::scene::{Align, Font, Paint, Primitive};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Renderer as _, Text};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell};
use iced::{
    Color, Element, Event, Length, Pixels, Point, Rectangle, Renderer, Size, Theme, alignment,
    mouse,
};

use crate::font;
use crate::theme::{DataColors, Tokens, data, tokens};

type Handler<'a, Message> = Box<dyn Fn(f32) -> Message + 'a>;

pub struct Scene<'a, Message> {
    primitives: Vec<Primitive>,
    height: f32,
    on_press: Option<Handler<'a, Message>>,
    on_drag: Option<Handler<'a, Message>>,
    on_release: Option<Message>,
}

#[derive(Default)]
struct State {
    dragging: bool,
}

pub fn view<'a, Message>(primitives: Vec<Primitive>, height: f32) -> Scene<'a, Message> {
    Scene {
        primitives,
        height,
        on_press: None,
        on_drag: None,
        on_release: None,
    }
}

impl<'a, Message> Scene<'a, Message> {
    pub fn on_press(mut self, on_press: impl Fn(f32) -> Message + 'a) -> Self {
        self.on_press = Some(Box::new(on_press));
        self
    }

    pub fn on_drag(mut self, on_drag: impl Fn(f32) -> Message + 'a) -> Self {
        self.on_drag = Some(Box::new(on_drag));
        self
    }

    pub fn on_release(mut self, on_release: Message) -> Self {
        self.on_release = Some(on_release);
        self
    }
}

impl<'a, Message: Clone + 'a> From<Scene<'a, Message>> for Element<'a, Message> {
    fn from(scene: Scene<'a, Message>) -> Self {
        Element::new(scene)
    }
}

pub fn color(paint: Paint, tokens: &Tokens, data: &DataColors) -> Color {
    match paint {
        Paint::Base(base) => data.bases[base as usize],
        Paint::OnBase(base) => data.on_bases[base as usize],
        Paint::Muted => tokens.muted,
        Paint::Line => tokens.line,
        Paint::Accent => tokens.accent,
        Paint::Selection => tokens.accent_soft,
    }
}

fn typeface(font: Font) -> iced::Font {
    match font {
        Font::Ui => font::UI,
        Font::Mono => font::MONO,
        Font::MonoStrong => font::MONO_STRONG,
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Scene<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(self.height))
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Some(on_press) = &self.on_press else {
            return;
        };
        let bounds = layout.bounds();
        let share = |x: f32| ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_over(bounds) {
                    state.dragging = true;
                    shell.publish(on_press(share(position.x)));
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                if let Some(on_drag) = &self.on_drag {
                    shell.publish(on_drag(share(position.x)));
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                if let Some(on_release) = &self.on_release {
                    shell.publish(on_release.clone());
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_press.is_none() {
            mouse::Interaction::None
        } else if tree.state.downcast_ref::<State>().dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::None
        }
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
    use iced::{Element, Event, Point, Settings, mouse};
    use iced_test::{Simulator, selector, simulator};

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
        assert_eq!(
            color(Paint::Selection, &LIGHT, &LIGHT_DATA),
            LIGHT.accent_soft
        );
    }

    #[test]
    fn keeps_its_height() {
        let mut ui = simulator(container(view::<()>(Vec::new(), 20.0)).id("scene"));
        let scene = ui.find(selector::id("scene")).unwrap();
        assert!((scene.bounds().height - 20.0).abs() < f32::EPSILON);
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Probe {
        Pressed(f32),
        Dragged(f32),
        Released,
    }

    fn interactive<'a>() -> Element<'a, Probe> {
        view(Vec::new(), 12.0)
            .on_press(Probe::Pressed)
            .on_drag(Probe::Dragged)
            .on_release(Probe::Released)
            .into()
    }

    fn run(element: Element<'_, Probe>, at: Point, events: Vec<Event>) -> Vec<Probe> {
        let mut ui = Simulator::with_size(Settings::default(), (400.0, 300.0), element);
        ui.point_at(at);
        let _ = ui.simulate(events);
        ui.into_messages().collect()
    }

    fn press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    fn release() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }

    fn moved(x: f32, y: f32) -> Event {
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, y),
        })
    }

    #[test]
    fn presses_report_their_share_of_the_width() {
        let messages = run(
            interactive(),
            Point::new(100.0, 6.0),
            vec![press(), release()],
        );
        assert_eq!(messages, [Probe::Pressed(0.25), Probe::Released]);
    }

    #[test]
    fn drags_follow_the_pointer_until_release() {
        let events = vec![
            press(),
            moved(200.0, 150.0),
            moved(500.0, 150.0),
            release(),
            moved(300.0, 6.0),
        ];
        assert_eq!(
            run(interactive(), Point::new(100.0, 6.0), events),
            [
                Probe::Pressed(0.25),
                Probe::Dragged(0.5),
                Probe::Dragged(1.0),
                Probe::Released
            ]
        );
    }

    #[test]
    fn presses_outside_the_scene_are_ignored() {
        let messages = run(
            interactive(),
            Point::new(100.0, 100.0),
            vec![press(), release()],
        );
        assert_eq!(messages, []);
    }

    #[test]
    fn passive_scenes_ignore_the_pointer() {
        let passive: Element<'_, Probe> = view(Vec::new(), 12.0).into();
        let messages = run(passive, Point::new(100.0, 6.0), vec![press(), release()]);
        assert_eq!(messages, []);
    }
}
