use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse::{self, Click, ScrollDelta, click};
use iced::advanced::widget::{self, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::keyboard::{self, Modifiers};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

const LINE_HEIGHT: f32 = 40.0;
const ZOOM_RATE: f32 = 0.005;

type Callback<'a, Message, T> = Box<dyn Fn(T) -> Message + 'a>;

pub struct Navigator<'a, Message> {
    content: Element<'a, Message>,
    width: f32,
    inset: f32,
    on_pan: Option<Callback<'a, Message, f32>>,
    on_zoom: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    on_resize: Option<Callback<'a, Message, f32>>,
}

#[derive(Default)]
struct State {
    modifiers: Modifiers,
    drag: Option<f32>,
    click: Option<Click>,
}

impl<'a, Message> Navigator<'a, Message> {
    pub fn new(content: impl Into<Element<'a, Message>>, width: f32) -> Self {
        Self {
            content: content.into(),
            width,
            inset: 0.0,
            on_pan: None,
            on_zoom: None,
            on_resize: None,
        }
    }

    pub fn inset(mut self, inset: f32) -> Self {
        self.inset = inset;
        self
    }

    pub fn on_pan(mut self, on_pan: impl Fn(f32) -> Message + 'a) -> Self {
        self.on_pan = Some(Box::new(on_pan));
        self
    }

    pub fn on_zoom(mut self, on_zoom: impl Fn(f32, f32) -> Message + 'a) -> Self {
        self.on_zoom = Some(Box::new(on_zoom));
        self
    }

    pub fn on_resize(mut self, on_resize: impl Fn(f32) -> Message + 'a) -> Self {
        self.on_resize = Some(Box::new(on_resize));
        self
    }

    fn pan(&self, shell: &mut Shell<'_, Message>, dx: f32) {
        if let Some(on_pan) = &self.on_pan {
            shell.publish(on_pan(dx));
        }
        shell.capture_event();
    }

    fn zoom(&self, shell: &mut Shell<'_, Message>, factor: f32, anchor: f32) {
        if let Some(on_zoom) = &self.on_zoom {
            shell.publish(on_zoom(factor, anchor));
        }
        shell.capture_event();
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Navigator<'_, Message> {
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
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let width = bounds.width - 2.0 * self.inset;
        if (width - self.width).abs() > 0.5
            && let Some(on_resize) = &self.on_resize
        {
            shell.publish(on_resize(width));
        }
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(position) = cursor.position_over(bounds) {
                    let (dx, dy) = match *delta {
                        ScrollDelta::Lines { x, y } => (x * LINE_HEIGHT, y * LINE_HEIGHT),
                        ScrollDelta::Pixels { x, y } => (x, y),
                    };
                    if state.modifiers.command() {
                        let anchor = position.x - bounds.x - self.inset;
                        return self.zoom(shell, (dy * ZOOM_RATE).exp(), anchor);
                    }
                    let dx = if state.modifiers.shift() { dy } else { dx };
                    if dx.abs() > f32::EPSILON {
                        return self.pan(shell, dx);
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(last) = state.drag.replace(position.x) {
                    return self.pan(shell, position.x - last);
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag = None;
            }
            _ => {}
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.is_event_captured() {
            return;
        }
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event
            && let Some(position) = cursor.position_over(bounds)
        {
            let state = tree.state.downcast_mut::<State>();
            let click = Click::new(position, mouse::Button::Left, state.click);
            state.click = Some(click);
            if click.kind() == click::Kind::Double {
                let anchor = position.x - bounds.x - self.inset;
                self.zoom(shell, 2.0, anchor);
            } else {
                state.drag = Some(position.x);
                shell.capture_event();
            }
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
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<State>().drag.is_some() {
            return mouse::Interaction::Grabbing;
        }
        let interaction = self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        );
        if interaction == mouse::Interaction::None && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Grab
        } else {
            interaction
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<Navigator<'a, Message>> for Element<'a, Message> {
    fn from(navigator: Navigator<'a, Message>) -> Self {
        Element::new(navigator)
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::{self, Modifiers};
    use iced::mouse::{self, ScrollDelta};
    use iced::widget::space;
    use iced::{Event, Length, Point, Settings};
    use iced_test::Simulator;

    use super::Navigator;

    #[derive(Debug, Clone, PartialEq)]
    enum Probe {
        Pan(f32),
        Zoom(f32, f32),
        Resized(f32),
    }

    fn run(width: f32, at: Point, events: Vec<Event>) -> Vec<Probe> {
        let navigator = Navigator::new(space().width(Length::Fill).height(Length::Fill), width)
            .inset(10.0)
            .on_pan(Probe::Pan)
            .on_zoom(Probe::Zoom)
            .on_resize(Probe::Resized);
        let mut ui = Simulator::with_size(Settings::default(), (400.0, 300.0), navigator);
        ui.point_at(at);
        let _ = ui.simulate(events);
        ui.into_messages().collect()
    }

    fn modifiers(modifiers: Modifiers) -> Event {
        Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers))
    }

    fn wheel(y: f32) -> Event {
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: ScrollDelta::Lines { x: 0.0, y },
        })
    }

    fn press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    fn release() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }

    #[test]
    fn command_wheel_zooms_at_the_cursor() {
        let messages = run(
            380.0,
            Point::new(110.0, 50.0),
            vec![modifiers(Modifiers::COMMAND), wheel(1.0)],
        );
        let [Probe::Zoom(factor, anchor)] = messages.as_slice() else {
            panic!("expected one zoom, got {messages:?}");
        };
        assert!(*factor > 1.0);
        assert!((anchor - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn drag_pans_with_the_pointer() {
        let moved = Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(150.0, 50.0),
        });
        let messages = run(
            380.0,
            Point::new(100.0, 50.0),
            vec![press(), moved, release()],
        );
        assert_eq!(messages, [Probe::Pan(50.0)]);
    }

    #[test]
    fn plain_wheel_is_left_for_scrolling() {
        assert_eq!(run(380.0, Point::new(100.0, 50.0), vec![wheel(1.0)]), []);
    }

    #[test]
    fn shift_wheel_pans() {
        let messages = run(
            380.0,
            Point::new(100.0, 50.0),
            vec![modifiers(Modifiers::SHIFT), wheel(1.0)],
        );
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Probe::Pan(dx) if dx > 0.0));
    }

    #[test]
    fn double_click_zooms_in_twice_as_far() {
        let messages = run(
            380.0,
            Point::new(110.0, 50.0),
            vec![press(), release(), press(), release()],
        );
        assert_eq!(messages, [Probe::Zoom(2.0, 100.0)]);
    }

    #[test]
    fn reports_data_width_when_it_differs() {
        let messages = run(100.0, Point::new(100.0, 50.0), vec![release()]);
        assert_eq!(messages, [Probe::Resized(380.0)]);
    }
}
