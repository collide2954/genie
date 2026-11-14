use iced::widget::{Column, button, center, column, container, opaque, row, space, text};
use iced::{Alignment, Element, Length};

use crate::app::Message;
use crate::chrome::{keys, modifier};
use crate::icon::{self, Icon};
use crate::shortcuts::Command;
use crate::{font, style};

const WIDTH: f32 = 720.0;

pub struct Group {
    pub title: &'static str,
    pub rows: Vec<(String, &'static str)>,
}

fn group(title: &'static str, rows: Vec<(String, &'static str)>) -> Group {
    Group { title, rows }
}

pub fn groups() -> [Group; 6] {
    let (back, forward) = if cfg!(target_os = "macos") {
        ("Cmd+[", "Cmd+]")
    } else {
        ("Alt+Left", "Alt+Right")
    };
    [
        group(
            "Move",
            vec![
                ("Drag the tracks".to_owned(), "Move along the sequence"),
                ("Left, Right".to_owned(), "Move a quarter view"),
                ("Shift+Left, Shift+Right".to_owned(), "Move a full view"),
                (format!("{back}, {forward}"), "Go back or forward"),
                (keys('L'), "Go to a locus or a name"),
            ],
        ),
        group(
            "Zoom",
            vec![
                (format!("{}+wheel, + or -", modifier()), "Zoom in or out"),
                ("Drag on the ruler".to_owned(), "Zoom to a range"),
            ],
        ),
        group(
            "Regions",
            vec![
                (keys('D'), "Add the view"),
                ("Shift+drag on the ruler".to_owned(), "Add a range"),
            ],
        ),
        group(
            "Tracks",
            vec![
                ("Right-click a track".to_owned(), "Track menu"),
                ("Drag its name".to_owned(), "Move a track"),
                ("Double-click its name".to_owned(), "Rename a track"),
                ("Drag the bottom edge".to_owned(), "Set the height"),
                ("Double-click the edge".to_owned(), "Show all rows"),
                (
                    format!("{}, {}", keys('F'), keys('B')),
                    "Next or previous feature",
                ),
                ("Click the card".to_owned(), "Keep a card open"),
                ("Right-click the sequence".to_owned(), "Strand and frames"),
            ],
        ),
        group(
            "Files and images",
            vec![
                ("Drop them on genie".to_owned(), "Open files"),
                (keys('U'), "Open a URL"),
                (keys('S'), "Save the session"),
                (keys('E'), "Export an image"),
                (keys('C'), "Copy the locus or a card"),
                ("Right-click the ruler".to_owned(), "Copy the sequence"),
            ],
        ),
        group(
            "Help",
            vec![
                ("F1".to_owned(), "Show this list"),
                ("Escape".to_owned(), "Close menus and cards"),
            ],
        ),
    ]
}

fn section<'a>(group: Group) -> Column<'a, Message> {
    let rows = group.rows.into_iter().map(|(keys, action)| {
        row![
            text(action).size(12).width(Length::Fill),
            text(keys).size(11).style(style::muted),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
    });
    column![text(group.title).size(11).style(style::muted)]
        .extend(rows)
        .spacing(4)
}

pub fn view<'a>() -> Element<'a, Message> {
    let [moving, zoom, regions, tracks, files, help] = groups();
    let close = container(
        button(icon::view(Icon::Close, 10.0))
            .width(20)
            .height(20)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Command(Command::Help)),
    )
    .id("help-close");
    let title = row![
        text("Keys and mouse").font(font::UI_STRONG).size(13),
        space::horizontal(),
        close
    ]
    .align_y(Alignment::Center);
    let left = column![section(moving), section(zoom), section(regions)].spacing(14);
    let right = column![section(tracks), section(files), section(help)].spacing(14);
    let body = row![
        left.width(Length::FillPortion(1)),
        right.width(Length::FillPortion(1))
    ]
    .spacing(32);
    let card = container(column![title, body].spacing(14))
        .id("help-dialog")
        .width(WIDTH)
        .padding(16)
        .style(style::menu);
    center(opaque(card)).into()
}

#[cfg(test)]
mod tests {
    use super::groups;
    use crate::chrome::keys;

    #[test]
    fn groups_name_each_key_once() {
        let rows: Vec<String> = groups()
            .into_iter()
            .flat_map(|group| group.rows)
            .map(|(keys, _)| keys)
            .collect();
        let mut unique = rows.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), rows.len());
        assert!(rows.contains(&keys('D')));
    }
}
