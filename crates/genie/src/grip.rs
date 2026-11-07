use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, mouse};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub struct Grip<'a, Message> {
    content: Element<'a, Message>,
    on_drag: Box<dyn Fn(f32) -> Message + 'a>,
    on_release: Message,
    on_double_click: Option<Message>,
    interaction: mouse::Interaction,
}

#[derive(Debug, Default)]
struct State {
    origin: Option<f32>,
    moved: bool,
    pressed: Option<Instant>,
}

impl<'a, Message> Grip<'a, Message> {
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        on_drag: impl Fn(f32) -> Message + 'a,
        on_release: Message,
    ) -> Self {
        Self {
            content: content.into(),
            on_drag: Box::new(on_drag),
            on_release,
            on_double_click: None,
            interaction: mouse::Interaction::Grab,
        }
    }

    pub fn on_double_click(mut self, message: Message) -> Self {
        self.on_double_click = Some(message);
        self
    }

    pub fn interaction(mut self, interaction: mouse::Interaction) -> Self {
        self.interaction = interaction;
        self
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for Grip<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        let state = tree.state.downcast_mut::<State>();
        let Event::Mouse(event) = event else {
            return;
        };
        match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let Some(position) = cursor.position_over(layout.bounds()) else {
                    return;
                };
                let now = Instant::now();
                let twice = state
                    .pressed
                    .is_some_and(|pressed| now.duration_since(pressed) < DOUBLE_CLICK);
                if let (true, Some(message)) = (twice, &self.on_double_click) {
                    shell.publish(message.clone());
                    state.pressed = None;
                } else {
                    state.pressed = Some(now);
                    state.origin = Some(position.y);
                    state.moved = false;
                }
                shell.capture_event();
            }
            mouse::Event::CursorMoved { .. } => {
                if let (Some(origin), Some(position)) = (state.origin, cursor.position()) {
                    state.moved = true;
                    shell.publish((self.on_drag)(position.y - origin));
                    shell.capture_event();
                }
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) if state.origin.take().is_some() => {
                if state.moved {
                    shell.publish(self.on_release.clone());
                }
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        if state.origin.is_some() || cursor.is_over(layout.bounds()) {
            self.interaction
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a, Message: Clone + 'a> From<Grip<'a, Message>> for Element<'a, Message> {
    fn from(grip: Grip<'a, Message>) -> Self {
        Element::new(grip)
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::{column, space};
    use iced::{Element, Event, Length, Point, Settings, mouse};
    use iced_test::Simulator;

    use super::Grip;

    #[derive(Debug, Clone, PartialEq)]
    enum Moved {
        By(f32),
        Released,
        Reset,
    }

    fn grip<'a>() -> Element<'a, Moved> {
        let grip = Grip::new(
            space().width(Length::Fill).height(10),
            Moved::By,
            Moved::Released,
        )
        .on_double_click(Moved::Reset);
        column![space().height(20), grip].into()
    }

    fn button(pressed: bool) -> Event {
        let button = mouse::Button::Left;
        Event::Mouse(if pressed {
            mouse::Event::ButtonPressed(button)
        } else {
            mouse::Event::ButtonReleased(button)
        })
    }

    fn run(steps: &[(Point, Option<bool>)]) -> Vec<Moved> {
        let mut ui = Simulator::with_size(Settings::default(), (200.0, 100.0), grip());
        for (at, press) in steps {
            ui.point_at(*at);
            let event = match press {
                Some(pressed) => button(*pressed),
                None => Event::Mouse(mouse::Event::CursorMoved { position: *at }),
            };
            let _ = ui.simulate([event]);
        }
        ui.into_messages().collect()
    }

    #[test]
    fn drags_report_the_distance_from_the_press() {
        let messages = run(&[
            (Point::new(50.0, 25.0), Some(true)),
            (Point::new(50.0, 60.0), None),
            (Point::new(80.0, 15.0), None),
            (Point::new(80.0, 15.0), Some(false)),
        ]);
        assert_eq!(
            messages,
            [Moved::By(35.0), Moved::By(-10.0), Moved::Released]
        );
    }

    #[test]
    fn presses_elsewhere_and_plain_clicks_do_nothing() {
        let elsewhere = run(&[
            (Point::new(50.0, 5.0), Some(true)),
            (Point::new(50.0, 60.0), None),
            (Point::new(50.0, 60.0), Some(false)),
        ]);
        assert_eq!(elsewhere, []);
        let click = run(&[
            (Point::new(50.0, 25.0), Some(true)),
            (Point::new(50.0, 25.0), Some(false)),
        ]);
        assert_eq!(click, []);
    }

    #[test]
    fn double_clicks_reset() {
        let at = Point::new(50.0, 25.0);
        let messages = run(&[
            (at, Some(true)),
            (at, Some(false)),
            (at, Some(true)),
            (at, Some(false)),
        ]);
        assert_eq!(messages, [Moved::Reset]);
    }
}
