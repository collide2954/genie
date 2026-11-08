use genie_core::Strand;
use genie_render::scene::{Align, Allele, Font, Paint, Primitive, Shade};
use iced::advanced::graphics::geometry::Renderer as _;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::text::{self, Paragraph as _, Renderer as _, Text};
use iced::advanced::widget::{Tree, Widget, tree};
use iced::advanced::{Clipboard, Renderer as _, Shell};
use iced::widget::canvas::{Frame, LineCap, LineJoin, Path, Stroke};
use iced::{
    Border, Color, Element, Event, Length, Pixels, Point, Rectangle, Renderer, Size, Theme,
    alignment, mouse,
};

use crate::font;
use crate::theme::{DataColors, Tokens, data, tokens};

pub const CHIP: f32 = 0.85;

const CLICK_SLOP: f32 = 3.0;
const FADED: f32 = 0.35;
const HALF: f32 = 0.5;
const PAIR: f32 = 0.85;
const CHIP_PADDING: f32 = 4.0;
const CHIP_LINE: f32 = 1.3;

type Handler<'a, Message> = Box<dyn Fn(f32) -> Message + 'a>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    pub local: Point,
    pub at: Point,
}

pub struct Scene<'a, Message> {
    primitives: Vec<Primitive>,
    height: f32,
    on_press: Option<Handler<'a, Message>>,
    on_drag: Option<Handler<'a, Message>>,
    on_release: Option<Message>,
    on_hover: Option<Box<dyn Fn(Option<Pointer>) -> Message + 'a>>,
    on_click: Option<Box<dyn Fn(Pointer) -> Message + 'a>>,
    crosshair: bool,
}

#[derive(Default)]
struct State {
    dragging: bool,
    hovered: bool,
    pressed: Option<Point>,
}

pub fn view<'a, Message>(primitives: Vec<Primitive>, height: f32) -> Scene<'a, Message> {
    Scene {
        primitives,
        height,
        on_press: None,
        on_drag: None,
        on_release: None,
        on_hover: None,
        on_click: None,
        crosshair: false,
    }
}

impl<'a, Message> Scene<'a, Message> {
    pub fn on_hover(mut self, on_hover: impl Fn(Option<Pointer>) -> Message + 'a) -> Self {
        self.on_hover = Some(Box::new(on_hover));
        self
    }

    pub fn on_click(mut self, on_click: impl Fn(Pointer) -> Message + 'a) -> Self {
        self.on_click = Some(Box::new(on_click));
        self
    }

    fn watch(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
        shell: &mut Shell<'_, Message>,
    ) {
        let pointer = |at: Point| Pointer {
            local: Point::new(at.x - bounds.x, at.y - bounds.y),
            at,
        };
        let over = cursor.position_over(bounds);
        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) => {
                let Some(on_hover) = &self.on_hover else {
                    return;
                };
                if let Some(at) = over {
                    state.hovered = true;
                    shell.publish(on_hover(Some(pointer(at))));
                } else if state.hovered {
                    state.hovered = false;
                    shell.publish(on_hover(None));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => state.pressed = over,
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(on_click) = &self.on_click
                    && let (Some(start), Some(at)) = (state.pressed.take(), over)
                    && start.distance(at) <= CLICK_SLOP
                {
                    shell.publish(on_click(pointer(at)));
                }
            }
            _ => {}
        }
    }
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

    pub fn crosshair(mut self) -> Self {
        self.crosshair = true;
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
        Paint::Allele(allele, shade) => {
            let color = match allele {
                Allele::Base(base) => data.bases[base as usize],
                Allele::Insertion => data.insertion,
                Allele::Deletion => tokens.muted,
                Allele::Other => tokens.text,
            };
            match shade {
                Shade::Full => color,
                Shade::Half => color.scale_alpha(HALF),
                Shade::Faded => color.scale_alpha(FADED),
            }
        }
        Paint::HomRef => data.codons[1],
        Paint::Base(base) => data.bases[base as usize],
        Paint::OnBase(base) => data.on_bases[base as usize],
        Paint::Faded(base) => data.bases[base as usize].scale_alpha(FADED),
        Paint::Hue(hue) => data.hues[hue as usize],
        Paint::Tint(hue) => data.hues[hue as usize].scale_alpha(FADED),
        Paint::Pair(hue) => data.hues[hue as usize].scale_alpha(PAIR),
        Paint::Rgb([red, green, blue]) => Color::from_rgb8(red, green, blue),
        Paint::Strand(Strand::Reverse) => data.strands[1],
        Paint::Strand(Strand::Forward | Strand::Unknown) => data.strands[0],
        Paint::Coverage => data.coverage,
        Paint::Insertion => data.insertion,
        Paint::Codon(even) => data.codons[usize::from(!even)],
        Paint::Text => tokens.text,
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
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        self.watch(state, event, bounds, cursor, shell);
        let Some(on_press) = &self.on_press else {
            return;
        };
        let share = |x: f32| ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
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
        let dragging = tree.state.downcast_ref::<State>().dragging;
        if self.on_press.is_none() {
            mouse::Interaction::None
        } else if self.crosshair && (dragging || cursor.is_over(layout.bounds())) {
            mouse::Interaction::Crosshair
        } else if dragging {
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
            let mut marks: Option<Frame> = None;
            for primitive in &self.primitives {
                match primitive {
                    Primitive::Chevron {
                        x,
                        y,
                        size,
                        forward,
                        paint,
                    } => {
                        let frame =
                            marks.get_or_insert_with(|| Frame::with_bounds(renderer, bounds));
                        let reach = if *forward { size / 2.0 } else { -size / 2.0 };
                        let (x, y) = (bounds.x + x, bounds.y + y);
                        let path = Path::new(|builder| {
                            builder.move_to(Point::new(x - reach, y - size));
                            builder.line_to(Point::new(x + reach, y));
                            builder.line_to(Point::new(x - reach, y + size));
                        });
                        let stroke = Stroke::default()
                            .with_color(color(*paint, tokens, data))
                            .with_width(1.0)
                            .with_line_cap(LineCap::Round)
                            .with_line_join(LineJoin::Round);
                        frame.stroke(&path, stroke);
                    }
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
                                Align::End => text::Alignment::Right,
                            },
                            align_y: alignment::Vertical::Center,
                            shaping: text::Shaping::Basic,
                            wrapping: text::Wrapping::None,
                        },
                        Point::new(bounds.x + x, bounds.y + y),
                        color(*paint, tokens, data),
                        bounds,
                    ),
                    Primitive::Label {
                        x,
                        y,
                        content,
                        size,
                    } => label(renderer, bounds, Point::new(*x, *y), content, *size, tokens),
                }
            }
            if let Some(frame) = marks {
                renderer.draw_geometry(frame.into_geometry());
            }
        });
    }
}

fn label(
    renderer: &mut Renderer,
    bounds: Rectangle,
    at: Point,
    content: &str,
    size: f32,
    tokens: &Tokens,
) {
    let text = |content| Text {
        content,
        bounds: bounds.size(),
        size: Pixels(size),
        line_height: text::LineHeight::Relative(1.0),
        font: typeface(Font::Ui),
        align_x: text::Alignment::Right,
        align_y: alignment::Vertical::Center,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    };
    let width = <Renderer as text::Renderer>::Paragraph::with_text(text(content)).min_width();
    let height = (size * CHIP_LINE).round();
    let chip = Rectangle {
        x: bounds.x + at.x - width - CHIP_PADDING,
        y: bounds.y + at.y - height / 2.0,
        width: width + 2.0 * CHIP_PADDING,
        height,
    };
    renderer.fill_quad(
        Quad {
            bounds: chip,
            border: Border::default().rounded(3),
            ..Quad::default()
        },
        tokens.canvas.scale_alpha(CHIP),
    );
    renderer.fill_text(
        text(content).with_content(content.to_owned()),
        Point::new(bounds.x + at.x, bounds.y + at.y),
        tokens.muted,
        bounds,
    );
}

#[cfg(test)]
mod tests {
    use genie_core::Strand;
    use genie_render::scene::{Base, Hue, Paint};
    use iced::widget::container;
    use iced::{Element, Event, Point, Settings, mouse};
    use iced_test::{Simulator, selector, simulator};

    use super::{Pointer, color, view};
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
    fn hue_paints_use_data_colors() {
        assert_eq!(
            color(Paint::Hue(Hue::Blue), &LIGHT, &LIGHT_DATA),
            LIGHT_DATA.hues[0]
        );
        assert_eq!(
            color(Paint::Hue(Hue::Purple), &DARK, &DARK_DATA),
            DARK_DATA.hues[4]
        );
        assert_eq!(
            color(Paint::Tint(Hue::Green), &LIGHT, &LIGHT_DATA),
            LIGHT_DATA.hues[1].scale_alpha(0.35)
        );
        assert_eq!(
            color(Paint::Pair(Hue::Vermillion), &DARK, &DARK_DATA),
            DARK_DATA.hues[3].scale_alpha(0.85)
        );
    }

    #[test]
    fn read_paints_use_data_colors() {
        assert_eq!(
            color(Paint::Strand(Strand::Forward), &LIGHT, &LIGHT_DATA),
            LIGHT_DATA.strands[0]
        );
        assert_eq!(
            color(Paint::Strand(Strand::Reverse), &DARK, &DARK_DATA),
            DARK_DATA.strands[1]
        );
        assert_eq!(
            color(Paint::Coverage, &DARK, &DARK_DATA),
            DARK_DATA.coverage
        );
        assert_eq!(
            color(Paint::Insertion, &LIGHT, &LIGHT_DATA),
            LIGHT_DATA.insertion
        );
        assert_eq!(
            color(Paint::Faded(Base::C), &LIGHT, &LIGHT_DATA),
            LIGHT_DATA.bases[1].scale_alpha(0.35)
        );
    }

    #[test]
    fn chrome_paints_use_theme_tokens() {
        assert_eq!(color(Paint::Text, &DARK, &DARK_DATA), DARK.text);
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

    #[derive(Debug, Clone, PartialEq)]
    enum Seen {
        Hovered(Option<Pointer>),
        Clicked(Pointer),
    }

    fn watched(steps: Vec<(Point, Event)>) -> Vec<Seen> {
        let watched = view(Vec::new(), 40.0)
            .on_hover(Seen::Hovered)
            .on_click(Seen::Clicked);
        let element: Element<'_, Seen> =
            iced::widget::column![iced::widget::space().height(10), watched].into();
        let mut ui = Simulator::with_size(Settings::default(), (400.0, 300.0), element);
        for (at, event) in steps {
            ui.point_at(at);
            let _ = ui.simulate([event]);
        }
        ui.into_messages().collect()
    }

    fn step(x: f32, y: f32, event: Event) -> (Point, Event) {
        (Point::new(x, y), event)
    }

    fn pointer(x: f32, y: f32) -> Pointer {
        Pointer {
            local: Point::new(x, y - 10.0),
            at: Point::new(x, y),
        }
    }

    #[test]
    fn hovering_reports_the_pointer_until_it_leaves() {
        let messages = watched(vec![
            step(50.0, 20.0, moved(50.0, 20.0)),
            step(60.0, 100.0, moved(60.0, 100.0)),
            step(70.0, 120.0, moved(70.0, 120.0)),
        ]);
        assert_eq!(
            messages,
            [
                Seen::Hovered(Some(pointer(50.0, 20.0))),
                Seen::Hovered(None)
            ]
        );
    }

    #[test]
    fn clicks_report_the_pointer() {
        let messages = watched(vec![step(50.0, 20.0, press()), step(51.0, 21.0, release())]);
        assert_eq!(messages, [Seen::Clicked(pointer(51.0, 21.0))]);
    }

    #[test]
    fn drags_are_not_clicks() {
        let messages = watched(vec![step(50.0, 20.0, press()), step(90.0, 20.0, release())]);
        assert_eq!(messages, []);
    }

    #[test]
    fn passive_scenes_ignore_the_pointer() {
        let passive: Element<'_, Probe> = view(Vec::new(), 12.0).into();
        let messages = run(passive, Point::new(100.0, 6.0), vec![press(), release()]);
        assert_eq!(messages, []);
    }
}
