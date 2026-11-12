use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, mouse};

pub struct Context<'a, Message> {
    content: Element<'a, Message>,
    on_open: Box<dyn Fn(Point) -> Message + 'a>,
    button: mouse::Button,
}

impl<'a, Message> Context<'a, Message> {
    pub fn new(
        content: impl Into<Element<'a, Message>>,
        on_open: impl Fn(Point) -> Message + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            on_open: Box::new(on_open),
            button: mouse::Button::Right,
        }
    }

    pub fn left(
        content: impl Into<Element<'a, Message>>,
        on_open: impl Fn(Point) -> Message + 'a,
    ) -> Self {
        Self {
            button: mouse::Button::Left,
            ..Self::new(content, on_open)
        }
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Context<'_, Message> {
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
        if let Event::Mouse(mouse::Event::ButtonPressed(button)) = event
            && *button == self.button
            && let Some(position) = cursor.position_over(layout.bounds())
        {
            shell.publish((self.on_open)(position));
            shell.capture_event();
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
        if self.button == mouse::Button::Left && cursor.is_over(layout.bounds()) {
            return mouse::Interaction::Pointer;
        }
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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

impl<'a, Message: 'a> From<Context<'a, Message>> for Element<'a, Message> {
    fn from(context: Context<'a, Message>) -> Self {
        Element::new(context)
    }
}

#[cfg(test)]
mod tests {
    use iced::widget::{column, space};
    use iced::{Element, Event, Length, Point, Settings, mouse};
    use iced_test::Simulator;

    use super::Context;

    #[derive(Debug, Clone, PartialEq)]
    struct Opened(Point);

    fn press(target: Context<'_, Opened>, at: Point, button: mouse::Button) -> Vec<Opened> {
        let element: Element<'_, Opened> = column![space().height(20), target].into();
        let mut ui = Simulator::with_size(Settings::default(), (200.0, 100.0), element);
        ui.point_at(at);
        let _ = ui.simulate(vec![
            Event::Mouse(mouse::Event::ButtonPressed(button)),
            Event::Mouse(mouse::Event::ButtonReleased(button)),
        ]);
        ui.into_messages().collect()
    }

    fn run(at: Point, button: mouse::Button) -> Vec<Opened> {
        press(
            Context::new(space().width(Length::Fill).height(40), Opened),
            at,
            button,
        )
    }

    #[test]
    fn left_contexts_report_left_presses() {
        let target = || Context::left(space().width(Length::Fill).height(40), Opened);
        let at = Point::new(30.0, 35.0);
        assert_eq!(press(target(), at, mouse::Button::Left), [Opened(at)]);
        assert_eq!(press(target(), at, mouse::Button::Right), []);
    }

    #[test]
    fn right_presses_report_the_window_position() {
        let at = Point::new(30.0, 35.0);
        assert_eq!(run(at, mouse::Button::Right), [Opened(at)]);
    }

    #[test]
    fn other_presses_and_places_are_ignored() {
        assert_eq!(run(Point::new(30.0, 35.0), mouse::Button::Left), []);
        assert_eq!(run(Point::new(30.0, 80.0), mouse::Button::Right), []);
    }
}
