use std::time::Duration;

use iced::widget::{button, column, container, mouse_area, row, space, text, text_input, tooltip};
use iced::{Alignment, Border, Element, Length, Padding, Size, Task, mouse, window};

use crate::app::Message;
use crate::icon::{self, Icon};
use crate::shortcuts::Command;
use crate::theme::tokens;
use crate::{font, style};

pub const BAR_HEIGHT: f32 = 32.0;
pub const LOCUS_WIDTH: f32 = 240.0;
const TOOL_SIZE: f32 = 24.0;
const TOOL_GAP: f32 = 2.0;
const TOOL_ICON: f32 = 14.0;
const WINDOW_BUTTON_WIDTH: f32 = 46.0;
const WINDOW_ICON: f32 = 16.0;
const GRIP: f32 = 6.0;
const BRAND_WIDTH: f32 = 60.0;
const BRAND_GAP: f32 = 12.0;
const NAVIGATION_GAP: f32 = 6.0;
const END_GAP: f32 = 6.0;
const TIP_DELAY: Duration = Duration::from_millis(400);

const fn inset(controls: Controls) -> f32 {
    match controls {
        Controls::Native => 76.0,
        Controls::Custom => 10.0,
    }
}

pub const fn locus_x(controls: Controls) -> f32 {
    inset(controls) + BRAND_WIDTH + BRAND_GAP + TOOL_SIZE + NAVIGATION_GAP
}

pub const fn settings_inset(controls: Controls) -> f32 {
    let buttons = match controls {
        Controls::Native => 0.0,
        Controls::Custom => 3.0 * WINDOW_BUTTON_WIDTH,
    };
    TOOL_GAP + TOOL_SIZE + END_GAP + buttons
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Controls {
    Native,
    Custom,
}

impl Controls {
    pub const CURRENT: Self = if cfg!(target_os = "macos") {
        Self::Native
    } else {
        Self::Custom
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Drag,
    Resize(Edge),
    Minimize,
    ToggleMaximize,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}

impl Edge {
    fn direction(self) -> window::Direction {
        match self {
            Self::North => window::Direction::North,
            Self::South => window::Direction::South,
            Self::East => window::Direction::East,
            Self::West => window::Direction::West,
            Self::NorthEast => window::Direction::NorthEast,
            Self::NorthWest => window::Direction::NorthWest,
            Self::SouthEast => window::Direction::SouthEast,
            Self::SouthWest => window::Direction::SouthWest,
        }
    }

    fn interaction(self) -> mouse::Interaction {
        match self {
            Self::North | Self::South => mouse::Interaction::ResizingVertically,
            Self::East | Self::West => mouse::Interaction::ResizingHorizontally,
            Self::NorthWest | Self::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
            Self::NorthEast | Self::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
        }
    }
}

pub fn perform(action: Action) -> Task<Message> {
    window::latest().and_then(move |id| match action {
        Action::Drag => window::drag(id),
        Action::Resize(edge) => window::drag_resize(id, edge.direction()),
        Action::Minimize => window::minimize(id, true),
        Action::ToggleMaximize => window::toggle_maximize(id),
        Action::Close => window::close(id),
    })
}

pub const WINDOW: Size = Size::new(1280.0, 800.0);

pub fn settings() -> window::Settings {
    window::Settings {
        size: WINDOW,
        min_size: Some(Size::new(720.0, 480.0)),
        decorations: Controls::CURRENT == Controls::Native,
        exit_on_close_request: false,
        platform_specific: platform(),
        ..window::Settings::default()
    }
}

#[cfg(target_os = "macos")]
fn platform() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        title_hidden: true,
        titlebar_transparent: true,
        fullsize_content_view: true,
    }
}

#[cfg(target_os = "windows")]
fn platform() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific {
        undecorated_shadow: true,
        corner_preference: window::settings::platform::CornerPreference::Round,
        ..window::settings::PlatformSpecific::default()
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform() -> window::settings::PlatformSpecific {
    window::settings::PlatformSpecific::default()
}

pub const LOCUS: &str = "locus";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocusField {
    pub value: String,
    pub error: Option<String>,
}

pub fn title_bar<'a>(
    controls: Controls,
    appearance: Icon,
    locus: Option<LocusField>,
) -> Element<'a, Message> {
    let logo = container(space())
        .width(14)
        .height(14)
        .style(|theme| container::Style {
            background: Some(tokens(theme).accent.into()),
            border: Border::default().rounded(4),
            ..container::Style::default()
        });
    let brand = container(
        row![logo, text("genie").font(font::UI_STRONG).size(12)]
            .spacing(6)
            .align_y(Alignment::Center),
    )
    .width(BRAND_WIDTH);
    let save = locus.is_some().then(|| {
        let words = format!("Save session ({})", keys('S'));
        tool(
            Icon::Save,
            "save",
            words,
            Message::Command(Command::SaveSession),
        )
    });
    let export = locus.is_some().then(|| {
        let words = format!("Export PNG ({})", keys('E'));
        tool(
            Icon::Camera,
            "export",
            words,
            Message::Command(Command::Export),
        )
    });
    let theme = match appearance {
        Icon::Sun => "Theme: light",
        Icon::Moon => "Theme: dark",
        _ => "Theme: system",
    };
    let navigation = row![tool(Icon::Open, "open", "Open a file", Message::Open)]
        .push(locus.map(locus_box))
        .spacing(NAVIGATION_GAP)
        .align_y(Alignment::Center);
    let tools = row![
        save,
        export,
        tool(Icon::Gear, "settings", "Settings", Message::ToggleSettings),
        tool(appearance, "appearance", theme, Message::CycleAppearance)
    ]
    .spacing(TOOL_GAP)
    .align_y(Alignment::Center);
    let mut bar = row![
        brand,
        space().width(BRAND_GAP),
        navigation,
        space::horizontal(),
        tools,
        space().width(END_GAP)
    ]
    .align_y(Alignment::Center)
    .height(BAR_HEIGHT)
    .padding(Padding {
        left: inset(controls),
        ..Padding::ZERO
    });
    if controls == Controls::Custom {
        bar = bar
            .push(window_button(Icon::Minimize, "minimize", Action::Minimize))
            .push(window_button(
                Icon::Maximize,
                "maximize",
                Action::ToggleMaximize,
            ))
            .push(window_button(Icon::Close, "close", Action::Close));
    }
    let bar = mouse_area(container(bar).style(|theme| container::background(tokens(theme).bar)))
        .on_press(Message::Window(Action::Drag))
        .on_double_click(Message::Window(Action::ToggleMaximize));
    let hairline = container(space())
        .width(Length::Fill)
        .height(1)
        .style(|theme| container::background(tokens(theme).line));
    column![bar, hairline].into()
}

pub fn resize_handles<'a>() -> Element<'a, Message> {
    let grip = |edge: Edge, width: Length, height: Length| -> Element<'a, Message> {
        mouse_area(space().width(width).height(height))
            .interaction(edge.interaction())
            .on_press(Message::Window(Action::Resize(edge)))
            .into()
    };
    let size = Length::Fixed(GRIP);
    column![
        row![
            grip(Edge::NorthWest, size, size),
            grip(Edge::North, Length::Fill, size),
            grip(Edge::NorthEast, size, size),
        ],
        row![
            grip(Edge::West, size, Length::Fill),
            space::horizontal(),
            grip(Edge::East, size, Length::Fill),
        ]
        .height(Length::Fill),
        row![
            grip(Edge::SouthWest, size, size),
            grip(Edge::South, Length::Fill, size),
            grip(Edge::SouthEast, size, size),
        ],
    ]
    .into()
}

fn locus_box<'a>(field: LocusField) -> Element<'a, Message> {
    let input = text_input("chr1:1,000-2,000", &field.value)
        .id(LOCUS)
        .font(font::MONO)
        .size(12)
        .padding([2, 8])
        .width(LOCUS_WIDTH)
        .style(style::locus)
        .on_input(Message::LocusEdited)
        .on_submit(Message::LocusSubmitted);
    let words = format!("Go to a locus, a name or a variant ID ({})", keys('L'));
    row![tip(input, words, tooltip::Position::Bottom)]
        .push(
            field
                .error
                .map(|error| text(error).size(11).style(style::muted)),
        )
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

pub fn keys(letter: char) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "Cmd"
    } else {
        "Ctrl"
    };
    format!("{modifier}+{letter}")
}

pub fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    words: impl Into<String>,
    position: tooltip::Position,
) -> Element<'a, Message> {
    let card = container(text(words.into()).size(11))
        .padding([3, 6])
        .style(style::menu);
    tooltip(content, card, position)
        .gap(4)
        .delay(TIP_DELAY)
        .into()
}

fn tool<'a>(
    icon: Icon,
    id: &'static str,
    words: impl Into<String>,
    message: Message,
) -> Element<'a, Message> {
    let button = button(icon::view(icon, TOOL_ICON))
        .width(TOOL_SIZE)
        .height(TOOL_SIZE)
        .padding(0)
        .style(style::ghost)
        .on_press(message);
    container(tip(button, words, tooltip::Position::Bottom))
        .id(id)
        .into()
}

fn window_button<'a>(icon: Icon, id: &'static str, action: Action) -> Element<'a, Message> {
    let words = match action {
        Action::Minimize => "Minimize",
        Action::ToggleMaximize => "Maximize or restore",
        _ => "Close",
    };
    let button = button(icon::view(icon, WINDOW_ICON))
        .width(WINDOW_BUTTON_WIDTH)
        .height(BAR_HEIGHT)
        .padding(0)
        .style(style::window_control)
        .on_press(Message::Window(action));
    container(tip(button, words, tooltip::Position::Bottom))
        .id(id)
        .into()
}

#[cfg(test)]
mod tests {
    use iced::Point;
    use iced::keyboard::key::Named;
    use iced_test::selector;
    use iced_test::simulator::{self, simulator};

    use super::{Action, Controls, Edge, LOCUS, LocusField, locus_x, resize_handles, title_bar};
    use crate::app::Message;
    use crate::icon::Icon;
    use crate::shortcuts::Command;

    fn press_at(element: iced::Element<'_, Message>, point: Point) -> Vec<Message> {
        let mut ui =
            iced_test::Simulator::with_size(iced::Settings::default(), (400.0, 300.0), element);
        ui.point_at(point);
        let _ = ui.simulate(simulator::click());
        ui.into_messages().collect()
    }

    #[test]
    fn window_buttons_send_window_actions() {
        for (id, action) in [
            ("minimize", Action::Minimize),
            ("maximize", Action::ToggleMaximize),
            ("close", Action::Close),
        ] {
            let mut ui = simulator(title_bar(Controls::Custom, Icon::Monitor, None));
            ui.click(selector::id(id)).unwrap();
            assert_eq!(
                ui.into_messages().collect::<Vec<_>>(),
                [Message::Window(action)]
            );
        }
    }

    #[test]
    fn bar_is_thirty_two_pixels_tall() {
        let mut ui = simulator(title_bar(Controls::Custom, Icon::Monitor, None));
        let close = ui.find(selector::id("close")).unwrap();
        assert!((close.bounds().height - 32.0).abs() < 0.5);
    }

    #[test]
    fn open_button_asks_for_a_file() {
        let mut ui = simulator(title_bar(Controls::Custom, Icon::Monitor, None));
        ui.click(selector::id("open")).unwrap();
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), [Message::Open]);
    }

    fn field(value: &str, error: Option<&str>) -> LocusField {
        LocusField {
            value: value.to_owned(),
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn shows_the_locus_when_there_is_one() {
        let mut ui = simulator(title_bar(
            Controls::Custom,
            Icon::Monitor,
            Some(field("chr1:1-4", None)),
        ));
        assert!(ui.find("chr1:1-4").is_ok());
    }

    #[test]
    fn locus_field_starts_at_the_exported_position() {
        for controls in [Controls::Custom, Controls::Native] {
            let mut ui = simulator(title_bar(
                controls,
                Icon::Monitor,
                Some(field("chr1", None)),
            ));
            let bounds = ui.find(selector::id(LOCUS)).unwrap().bounds();
            assert!((bounds.x - locus_x(controls)).abs() < 0.5, "{controls:?}");
        }
    }

    #[test]
    fn locus_box_reports_edits_and_submits() {
        let mut ui = simulator(title_bar(
            Controls::Custom,
            Icon::Monitor,
            Some(field("chr1:1-4", None)),
        ));
        ui.click(selector::id(LOCUS)).unwrap();
        let _ = ui.typewrite("0");
        let _ = ui.tap_key(Named::Enter);
        let messages: Vec<Message> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::LocusEdited(_)))
        );
        assert_eq!(messages.last(), Some(&Message::LocusSubmitted));
    }

    #[test]
    fn locus_errors_show_beside_the_box() {
        let error = "The assembly has no contig \"chr9\".";
        let mut ui = simulator(title_bar(
            Controls::Custom,
            Icon::Monitor,
            Some(field("chr9", Some(error))),
        ));
        assert!(ui.find(error).is_ok());
    }

    #[test]
    fn tips_name_the_shortcut_keys() {
        let expected = if cfg!(target_os = "macos") {
            "Cmd+E"
        } else {
            "Ctrl+E"
        };
        assert_eq!(super::keys('E'), expected);
    }

    #[test]
    fn save_button_shows_with_a_locus() {
        let mut ui = simulator(title_bar(
            Controls::Custom,
            Icon::Monitor,
            Some(field("chr1:1-4", None)),
        ));
        ui.click(selector::id("save")).unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [Message::Command(Command::SaveSession)]
        );
        let mut ui = simulator(title_bar(Controls::Custom, Icon::Monitor, None));
        assert!(ui.find(selector::id("save")).is_err());
    }

    #[test]
    fn settings_button_toggles_the_panel() {
        let mut ui = simulator(title_bar(Controls::Custom, Icon::Monitor, None));
        ui.click(selector::id("settings")).unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [Message::ToggleSettings]
        );
    }

    #[test]
    fn export_button_shows_with_a_locus() {
        let mut ui = simulator(title_bar(
            Controls::Custom,
            Icon::Monitor,
            Some(field("chr1:1-4", None)),
        ));
        ui.click(selector::id("export")).unwrap();
        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            [Message::Command(Command::Export)]
        );
        let mut ui = simulator(title_bar(Controls::Custom, Icon::Monitor, None));
        assert!(ui.find(selector::id("export")).is_err());
    }

    #[test]
    fn native_controls_have_no_window_buttons() {
        let mut ui = simulator(title_bar(Controls::Native, Icon::Monitor, None));
        assert!(ui.find(selector::id("close")).is_err());
    }

    #[test]
    fn pressing_empty_bar_space_drags_the_window() {
        let messages = press_at(
            title_bar(Controls::Custom, Icon::Monitor, None),
            Point::new(180.0, 20.0),
        );
        assert!(messages.contains(&Message::Window(Action::Drag)));
    }

    #[test]
    fn edges_and_corners_resize_the_window() {
        for (point, edge) in [
            (Point::new(200.0, 2.0), Edge::North),
            (Point::new(2.0, 150.0), Edge::West),
            (Point::new(398.0, 298.0), Edge::SouthEast),
        ] {
            assert_eq!(
                press_at(resize_handles(), point),
                [Message::Window(Action::Resize(edge))]
            );
        }
        assert_eq!(press_at(resize_handles(), Point::new(200.0, 150.0)), []);
    }
}
