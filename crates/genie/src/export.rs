use std::fs::File;
use std::io::{self, BufWriter};
use std::path::PathBuf;

use genie_core::Region;
use genie_formats::png;
use iced::advanced::renderer::{Headless, Style};
use iced::advanced::widget::Tree;
use iced::advanced::{Layout, Renderer as _, layout};
use iced::futures::executor::block_on;
use iced::theme::Base;
use iced::{Element, Pixels, Rectangle, Renderer, Size, Theme, mouse};

use crate::font;

pub const LIMIT: f32 = 8_192.0;
const SCALE: f32 = 2.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub cut: bool,
}

pub fn render<Message>(
    mut element: Element<'_, Message>,
    width: f32,
    theme: &Theme,
) -> Option<Image> {
    let mut renderer = block_on(<Renderer as Headless>::new(
        font::UI,
        Pixels(16.0),
        Some("tiny-skia"),
    ))?;
    let mut tree = Tree::new(element.as_widget());
    let limits = layout::Limits::new(Size::ZERO, Size::new(width, LIMIT + 1.0));
    let node = element
        .as_widget_mut()
        .layout(&mut tree, &renderer, &limits);
    let cut = node.size().height > LIMIT;
    let bounds = Rectangle::with_size(Size::new(node.size().width, node.size().height.min(LIMIT)));
    let size = Size::new(
        (bounds.width * SCALE).round() as u32,
        (bounds.height * SCALE).round() as u32,
    );
    if size.width == 0 || size.height == 0 {
        return None;
    }
    let base = theme.base();
    renderer.reset(bounds);
    element.as_widget().draw(
        &tree,
        &mut renderer,
        theme,
        &Style {
            text_color: base.text_color,
        },
        Layout::new(&node),
        mouse::Cursor::Unavailable,
        &bounds,
    );
    let rgba = renderer.screenshot(size, SCALE, base.background_color);
    Some(Image {
        width: size.width,
        height: size.height,
        rgba,
        cut,
    })
}

pub fn safe(text: &str) -> String {
    text.chars()
        .map(|letter| {
            if letter.is_ascii_alphanumeric() || "._-".contains(letter) {
                letter
            } else {
                '_'
            }
        })
        .collect()
}

pub fn stem(contig: &str, region: Region) -> String {
    format!("{}_{}-{}", safe(contig), region.start() + 1, region.end())
}

pub fn file_name(contig: &str, region: Region) -> String {
    format!("{}.png", stem(contig, region))
}

pub fn save(path: PathBuf, image: &Image) -> Result<PathBuf, String> {
    let is_png = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    let path = if is_png {
        path
    } else {
        let mut text = path.into_os_string();
        text.push(".png");
        PathBuf::from(text)
    };
    let failed = |error: io::Error| format!("The image could not be saved. {error}");
    let file = File::create(&path).map_err(failed)?;
    png::write(BufWriter::new(file), image.width, image.height, &image.rgba).map_err(failed)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use genie_core::Region;
    use iced::widget::{column, container, space};
    use iced::{Color, Element, Length};

    use super::{Image, LIMIT, file_name, render, save};
    use crate::scratch::Scratch;
    use crate::theme;

    #[test]
    fn names_files_after_the_locus() {
        let region = Region::between(999, 2_000);
        assert_eq!(file_name("chr1", region), "chr1_1000-2000.png");
        assert_eq!(
            file_name("HLA-A*01:01", region),
            "HLA-A_01_01_1000-2000.png"
        );
    }

    #[test]
    fn saves_png_files() {
        let scratch = Scratch::new("export");
        let image = Image {
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255],
            cut: false,
        };
        let path = scratch.file("view.png", "");
        assert_eq!(save(path.clone(), &image).unwrap(), path);
        assert!(fs::read(&path).unwrap().starts_with(b"\x89PNG"));
        let bare = path.with_file_name("bare");
        let saved = save(bare.clone(), &image).unwrap();
        assert_eq!(saved, bare.with_file_name("bare.png"));
        assert!(saved.is_file());
    }

    fn block<'a>(height: f32, color: Color) -> Element<'a, ()> {
        container(space().width(Length::Fill).height(height))
            .style(move |_| container::background(color))
            .into()
    }

    #[test]
    fn renders_at_twice_the_size() {
        let red = Color::from_rgb8(200, 10, 20);
        let blue = Color::from_rgb8(10, 20, 200);
        let element = column![block(30.0, red), block(10.0, blue)];
        let image = render(element.into(), 100.0, &theme::light()).unwrap();
        assert_eq!((image.width, image.height), (200, 80));
        let rows: Vec<&[u8]> = image.rgba.chunks(200 * 4).collect();
        assert!(
            rows[..60]
                .concat()
                .chunks(4)
                .all(|pixel| pixel == [200, 10, 20, 255])
        );
        assert!(
            rows[60..]
                .concat()
                .chunks(4)
                .all(|pixel| pixel == [10, 20, 200, 255])
        );
    }

    #[test]
    fn empty_views_draw_nothing() {
        let empty: Element<'_, ()> = space().into();
        assert_eq!(render(empty, 100.0, &theme::light()), None);
    }

    #[test]
    fn cuts_very_tall_views() {
        let image = render(block(LIMIT * 2.0, Color::BLACK), 10.0, &theme::dark()).unwrap();
        assert_eq!(image.height, (LIMIT * 2.0) as u32);
        assert!(image.cut);
        let image = render(block(LIMIT, Color::BLACK), 10.0, &theme::dark()).unwrap();
        assert_eq!(image.height, (LIMIT * 2.0) as u32);
        assert!(!image.cut);
    }
}
