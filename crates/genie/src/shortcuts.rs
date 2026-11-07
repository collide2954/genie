use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

const STEP: f32 = 0.25;
const PAGE: f32 = 1.0;
const ZOOM: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Pan(f32),
    Zoom(f32),
    FocusLocus,
    NextSuggestion,
    PreviousSuggestion,
    Dismiss,
    Export,
    Diagnostics,
}

pub fn command(key: &Key, modifiers: Modifiers) -> Option<Command> {
    let step = if modifiers.shift() { PAGE } else { STEP };
    match key.as_ref() {
        Key::Named(Named::ArrowLeft) => Some(Command::Pan(step)),
        Key::Named(Named::ArrowRight) => Some(Command::Pan(-step)),
        Key::Named(Named::ArrowDown) => Some(Command::NextSuggestion),
        Key::Named(Named::ArrowUp) => Some(Command::PreviousSuggestion),
        Key::Named(Named::Escape) => Some(Command::Dismiss),
        Key::Named(Named::F12) => Some(Command::Diagnostics),
        Key::Character("+" | "=") => Some(Command::Zoom(ZOOM)),
        Key::Character("-" | "_") => Some(Command::Zoom(1.0 / ZOOM)),
        Key::Character("l" | "L") if modifiers.command() => Some(Command::FocusLocus),
        Key::Character("e" | "E") if modifiers.command() => Some(Command::Export),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use iced::keyboard::key::Named;
    use iced::keyboard::{Key, Modifiers};

    use super::{Command, command};

    fn character(text: &str) -> Key {
        Key::Character(text.into())
    }

    #[test]
    fn arrows_pan_a_quarter_or_a_page() {
        let left = Key::Named(Named::ArrowLeft);
        let right = Key::Named(Named::ArrowRight);
        assert_eq!(command(&left, Modifiers::empty()), Some(Command::Pan(0.25)));
        assert_eq!(
            command(&right, Modifiers::empty()),
            Some(Command::Pan(-0.25))
        );
        assert_eq!(command(&left, Modifiers::SHIFT), Some(Command::Pan(1.0)));
        assert_eq!(command(&right, Modifiers::SHIFT), Some(Command::Pan(-1.0)));
    }

    #[test]
    fn plus_and_minus_zoom_by_two() {
        for text in ["+", "="] {
            assert_eq!(
                command(&character(text), Modifiers::empty()),
                Some(Command::Zoom(2.0))
            );
        }
        for text in ["-", "_"] {
            assert_eq!(
                command(&character(text), Modifiers::empty()),
                Some(Command::Zoom(0.5))
            );
        }
    }

    #[test]
    fn command_l_focuses_the_locus() {
        assert_eq!(
            command(&character("l"), Modifiers::COMMAND),
            Some(Command::FocusLocus)
        );
        assert_eq!(command(&character("l"), Modifiers::empty()), None);
    }

    #[test]
    fn command_e_exports() {
        assert_eq!(
            command(&character("e"), Modifiers::COMMAND),
            Some(Command::Export)
        );
        assert_eq!(command(&character("e"), Modifiers::empty()), None);
    }

    #[test]
    fn f12_toggles_diagnostics() {
        assert_eq!(
            command(&Key::Named(Named::F12), Modifiers::empty()),
            Some(Command::Diagnostics)
        );
    }

    #[test]
    fn up_and_down_move_through_suggestions() {
        assert_eq!(
            command(&Key::Named(Named::ArrowDown), Modifiers::empty()),
            Some(Command::NextSuggestion)
        );
        assert_eq!(
            command(&Key::Named(Named::ArrowUp), Modifiers::empty()),
            Some(Command::PreviousSuggestion)
        );
    }

    #[test]
    fn escape_dismisses() {
        assert_eq!(
            command(&Key::Named(Named::Escape), Modifiers::empty()),
            Some(Command::Dismiss)
        );
    }

    #[test]
    fn other_keys_do_nothing() {
        assert_eq!(command(&character("q"), Modifiers::empty()), None);
        assert_eq!(command(&Key::Named(Named::Enter), Modifiers::empty()), None);
    }
}
