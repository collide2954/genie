use std::borrow::Cow;
use std::sync::PoisonError;

use iced::Font;
use iced::advanced::graphics::text::font_system;
use iced::font::Weight;

pub const UI: Font = Font::with_name("Inter");
pub const UI_STRONG: Font = Font {
    weight: Weight::Semibold,
    ..UI
};
pub const MONO: Font = Font::with_name("JetBrains Mono");
pub const MONO_STRONG: Font = Font {
    weight: Weight::Bold,
    ..MONO
};

pub const FILES: [(&[u8], Font); 4] = [
    (include_bytes!("../assets/fonts/Inter-Regular.ttf"), UI),
    (
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        UI_STRONG,
    ),
    (
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
        MONO,
    ),
    (
        include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
        MONO_STRONG,
    ),
];

pub fn load() {
    let mut system = font_system()
        .write()
        .unwrap_or_else(PoisonError::into_inner);
    for (bytes, _) in FILES {
        system.load_font(Cow::Borrowed(bytes));
    }
}

#[cfg(test)]
mod tests {
    use iced::font::{Family, Weight};

    use super::FILES;

    fn number(weight: Weight) -> u16 {
        match weight {
            Weight::Normal => 400,
            Weight::Semibold => 600,
            Weight::Bold => 700,
            other => panic!("unexpected weight {other:?}"),
        }
    }

    #[test]
    fn embedded_files_match_font_names_and_weights() {
        for (bytes, font) in FILES {
            let mut database = fontdb::Database::new();
            database.load_font_data(bytes.to_vec());
            let face = database.faces().next().unwrap();
            let Family::Name(name) = font.family else {
                panic!("font has no family name");
            };
            assert_eq!(face.families[0].0, name);
            assert_eq!(face.weight.0, number(font.weight));
        }
    }
}
