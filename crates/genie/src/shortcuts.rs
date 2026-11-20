use genie_core::review::Verdict;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

const STEP: f32 = 0.25;
const PAGE: f32 = 1.0;
const ZOOM: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Pan(f32),
    Zoom(f32),
    Whole,
    /// Show all main chromosomes side by side.
    Genome,
    Help,
    Open,
    Palette,
    Back,
    Forward,
    NextFeature,
    PreviousFeature,
    Find,
    AddRegion,
    OpenUrl,
    FocusLocus,
    NextSuggestion,
    PreviousSuggestion,
    Dismiss,
    Export,
    Diagnostics,
    SaveSession,
    Copy,
    /// Undoes the last work on tracks or region marks.
    Undo,
    /// Does the last undone work again.
    Redo,
    /// Shows the tracks panel, or hides it.
    TrackList,
    /// Marks the variant in review: T for true, F for false and U for not sure.
    Mark(Verdict),
    /// Goes to the next (N) or the previous (P) variant in review.
    Step(bool),
}

pub fn command(key: &Key, modifiers: Modifiers) -> Option<Command> {
    let step = if modifiers.shift() { PAGE } else { STEP };
    match key.as_ref() {
        Key::Named(Named::ArrowLeft) if modifiers.alt() => Some(Command::Back),
        Key::Named(Named::ArrowRight) if modifiers.alt() => Some(Command::Forward),
        Key::Character("[") if modifiers.command() => Some(Command::Back),
        Key::Character("]") if modifiers.command() => Some(Command::Forward),
        Key::Named(Named::ArrowLeft) => Some(Command::Pan(step)),
        Key::Named(Named::ArrowRight) => Some(Command::Pan(-step)),
        Key::Named(Named::ArrowDown) => Some(Command::NextSuggestion),
        Key::Named(Named::ArrowUp) => Some(Command::PreviousSuggestion),
        Key::Named(Named::Escape) => Some(Command::Dismiss),
        Key::Named(Named::F1) => Some(Command::Help),
        Key::Named(Named::F12) => Some(Command::Diagnostics),
        Key::Character("+" | "=") => Some(Command::Zoom(ZOOM)),
        Key::Character("-" | "_") => Some(Command::Zoom(1.0 / ZOOM)),
        Key::Character("l" | "L") if modifiers.command() => Some(Command::FocusLocus),
        Key::Character("f" | "F") if modifiers.command() && modifiers.shift() => {
            Some(Command::Find)
        }
        Key::Character("f" | "F") if modifiers.command() => Some(Command::NextFeature),
        Key::Character("t" | "T") if modifiers.command() && modifiers.shift() => {
            Some(Command::TrackList)
        }
        Key::Character("b" | "B") if modifiers.command() => Some(Command::PreviousFeature),
        Key::Character("d" | "D") if modifiers.command() => Some(Command::AddRegion),
        Key::Character("k" | "K") if modifiers.command() => Some(Command::Palette),
        Key::Character("o" | "O") if modifiers.command() => Some(Command::Open),
        Key::Character("u" | "U") if modifiers.command() => Some(Command::OpenUrl),
        Key::Character("e" | "E") if modifiers.command() => Some(Command::Export),
        Key::Character("s" | "S") if modifiers.command() => Some(Command::SaveSession),
        Key::Character("c" | "C") if modifiers.command() => Some(Command::Copy),
        Key::Character("z" | "Z") if modifiers.command() && modifiers.shift() => {
            Some(Command::Redo)
        }
        Key::Character("z" | "Z") if modifiers.command() => Some(Command::Undo),
        Key::Character("y" | "Y") if modifiers.command() => Some(Command::Redo),
        _ if modifiers.command() || modifiers.alt() => None,
        Key::Character("t" | "T") => Some(Command::Mark(Verdict::True)),
        Key::Character("f" | "F") => Some(Command::Mark(Verdict::False)),
        Key::Character("u" | "U") => Some(Command::Mark(Verdict::Unsure)),
        Key::Character("n" | "N") => Some(Command::Step(true)),
        Key::Character("p" | "P") => Some(Command::Step(false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use genie_core::review::Verdict;
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
    fn alt_arrows_and_brackets_step_through_history() {
        let left = Key::Named(Named::ArrowLeft);
        let right = Key::Named(Named::ArrowRight);
        assert_eq!(command(&left, Modifiers::ALT), Some(Command::Back));
        assert_eq!(command(&right, Modifiers::ALT), Some(Command::Forward));
        let back = command(&character("["), Modifiers::COMMAND);
        let forward = command(&character("]"), Modifiers::COMMAND);
        assert_eq!(
            (back, forward),
            (Some(Command::Back), Some(Command::Forward))
        );
        assert_eq!(command(&character("["), Modifiers::empty()), None);
    }

    #[test]
    fn command_f_and_b_jump_between_features() {
        let next = command(&character("f"), Modifiers::COMMAND);
        let previous = command(&character("b"), Modifiers::COMMAND);
        assert_eq!(
            (next, previous),
            (Some(Command::NextFeature), Some(Command::PreviousFeature))
        );
        assert_eq!(command(&character("b"), Modifiers::empty()), None);
    }

    #[test]
    fn plain_letters_mark_and_step_in_a_review() {
        let plain = |text| command(&character(text), Modifiers::empty());
        assert_eq!(plain("t"), Some(Command::Mark(Verdict::True)));
        assert_eq!(plain("f"), Some(Command::Mark(Verdict::False)));
        assert_eq!(plain("u"), Some(Command::Mark(Verdict::Unsure)));
        assert_eq!(plain("n"), Some(Command::Step(true)));
        assert_eq!(plain("p"), Some(Command::Step(false)));
        let shifted = command(&character("T"), Modifiers::SHIFT);
        assert_eq!(shifted, Some(Command::Mark(Verdict::True)));
        assert_eq!(command(&character("n"), Modifiers::ALT), None);
        let url = command(&character("u"), Modifiers::COMMAND);
        assert_eq!(url, Some(Command::OpenUrl));
    }

    #[test]
    fn command_shift_f_finds_a_sequence() {
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(command(&character("F"), shift), Some(Command::Find));
        assert_eq!(command(&character("f"), shift), Some(Command::Find));
        assert_eq!(
            command(&character("F"), Modifiers::COMMAND),
            Some(Command::NextFeature)
        );
    }

    #[test]
    fn command_d_adds_the_view_as_a_region() {
        assert_eq!(
            command(&character("d"), Modifiers::COMMAND),
            Some(Command::AddRegion)
        );
        assert_eq!(command(&character("d"), Modifiers::empty()), None);
    }

    #[test]
    fn command_z_undoes_and_command_y_or_shift_z_redoes() {
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(
            command(&character("z"), Modifiers::COMMAND),
            Some(Command::Undo)
        );
        assert_eq!(
            command(&character("y"), Modifiers::COMMAND),
            Some(Command::Redo)
        );
        assert_eq!(command(&character("Z"), shift), Some(Command::Redo));
        assert_eq!(command(&character("z"), Modifiers::empty()), None);
    }

    #[test]
    fn command_shift_t_shows_the_tracks_panel() {
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(command(&character("T"), shift), Some(Command::TrackList));
        assert_eq!(command(&character("t"), shift), Some(Command::TrackList));
        assert_eq!(command(&character("t"), Modifiers::COMMAND), None);
    }

    #[test]
    fn command_k_opens_the_palette() {
        assert_eq!(
            command(&character("k"), Modifiers::COMMAND),
            Some(Command::Palette)
        );
        assert_eq!(command(&character("k"), Modifiers::empty()), None);
    }

    #[test]
    fn command_o_opens_a_file() {
        assert_eq!(
            command(&character("o"), Modifiers::COMMAND),
            Some(Command::Open)
        );
        assert_eq!(command(&character("o"), Modifiers::empty()), None);
    }

    #[test]
    fn command_u_asks_for_an_address() {
        assert_eq!(
            command(&character("u"), Modifiers::COMMAND),
            Some(Command::OpenUrl)
        );
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
    fn command_c_copies() {
        assert_eq!(
            command(&character("c"), Modifiers::COMMAND),
            Some(Command::Copy)
        );
        assert_eq!(command(&character("c"), Modifiers::empty()), None);
    }

    #[test]
    fn command_s_saves_the_session() {
        assert_eq!(
            command(&character("s"), Modifiers::COMMAND),
            Some(Command::SaveSession)
        );
    }

    #[test]
    fn f1_shows_the_keys_and_mouse_list() {
        assert_eq!(
            command(&Key::Named(Named::F1), Modifiers::empty()),
            Some(Command::Help)
        );
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
