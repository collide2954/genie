use std::fmt::Write as _;

use genie_render::scene::{Align, Font, Primitive};
use iced::advanced::text::{self, Paragraph as _, Text};
use iced::{Color, Pixels, Renderer, Size, alignment};

use crate::font;
use crate::scene::{CHIP, CHIP_LINE, CHIP_PADDING, color};
use crate::theme::{DataColors, Tokens};

const UI: &str = "Inter, 'Segoe UI', Helvetica, Arial, sans-serif";
const MONO: &str = "'JetBrains Mono', Consolas, Menlo, monospace";
const DROP: f32 = 0.36;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    Ui,
    Mono,
    MonoStrong,
}

impl Face {
    fn attributes(self) -> (&'static str, &'static str) {
        match self {
            Self::Ui => (UI, ""),
            Self::Mono => (MONO, ""),
            Self::MonoStrong => (MONO, " font-weight=\"700\""),
        }
    }

    fn font(self) -> iced::Font {
        match self {
            Self::Ui => font::UI,
            Self::Mono => font::MONO,
            Self::MonoStrong => font::MONO_STRONG,
        }
    }
}

impl From<Font> for Face {
    fn from(font: Font) -> Self {
        match font {
            Font::Ui => Self::Ui,
            Font::Mono => Self::Mono,
            Font::MonoStrong => Self::MonoStrong,
        }
    }
}

pub fn width(content: &str, size: f32, face: Face) -> f32 {
    let text = Text {
        content,
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(size),
        line_height: text::LineHeight::Relative(1.0),
        font: face.font(),
        align_x: text::Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    };
    <Renderer as text::Renderer>::Paragraph::with_text(text).min_width()
}

pub struct Svg {
    width: f32,
    body: String,
    clips: usize,
}

impl Svg {
    pub fn new(width: f32) -> Self {
        Self {
            width,
            body: String::new(),
            clips: 0,
        }
    }

    pub fn clip(&mut self, (x, y, width, height): (f32, f32, f32, f32)) {
        self.clips += 1;
        let id = self.clips;
        let _ = writeln!(
            self.body,
            "<clipPath id=\"c{id}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>\n<g clip-path=\"url(#c{id})\">",
            number(x),
            number(y),
            number(width),
            number(height)
        );
    }

    pub fn unclip(&mut self) {
        self.body.push_str("</g>\n");
    }

    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        let _ = writeln!(
            self.body,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
            number(x),
            number(y),
            number(width),
            number(height),
            fill(color)
        );
    }

    pub fn rounded(
        &mut self,
        (x, y, width, height): (f32, f32, f32, f32),
        radius: f32,
        color: Color,
    ) {
        let _ = writeln!(
            self.body,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"{}/>",
            number(x),
            number(y),
            number(width),
            number(height),
            number(radius),
            fill(color)
        );
    }

    pub fn spans(
        &mut self,
        (x, center): (f32, f32),
        size: f32,
        parts: &[(&str, Face, Color)],
        gap: f32,
    ) {
        let _ = write!(
            self.body,
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\">",
            number(x),
            number(center + DROP * size),
            number(size)
        );
        for (index, (content, face, color)) in parts.iter().enumerate() {
            let shift = if index == 0 {
                String::new()
            } else {
                format!(" dx=\"{}\"", number(gap))
            };
            let (family, weight) = face.attributes();
            let _ = write!(
                self.body,
                "<tspan{shift} font-family=\"{family}\"{weight}{}>{}</tspan>",
                fill(*color),
                escape(content)
            );
        }
        self.body.push_str("</text>\n");
    }

    pub fn text(
        &mut self,
        (x, center): (f32, f32),
        content: &str,
        size: f32,
        face: Face,
        color: Color,
        align: Align,
    ) {
        let (family, weight) = face.attributes();
        let anchor = match align {
            Align::Start => "",
            Align::Center => " text-anchor=\"middle\"",
            Align::End => " text-anchor=\"end\"",
        };
        let _ = writeln!(
            self.body,
            "<text x=\"{}\" y=\"{}\" font-family=\"{family}\" font-size=\"{}\"{weight}{anchor}{}>{}</text>",
            number(x),
            number(center + DROP * size),
            number(size),
            fill(color),
            escape(content)
        );
    }

    pub fn stroke(&mut self, points: &[(f32, f32)], color: Color, width: f32) {
        let points: Vec<String> = points
            .iter()
            .map(|(x, y)| format!("{},{}", number(*x), number(*y)))
            .collect();
        let [red, green, blue, _] = color.into_rgba8();
        let opacity = if color.a < 1.0 {
            format!(" stroke-opacity=\"{}\"", number(color.a))
        } else {
            String::new()
        };
        let _ = writeln!(
            self.body,
            "<polyline points=\"{}\" fill=\"none\" stroke=\"#{red:02x}{green:02x}{blue:02x}\"{opacity} stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
            points.join(" "),
            number(width)
        );
    }

    pub fn finish(self, height: f32, background: Color) -> String {
        let (width, height) = (number(self.width), number(height));
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n<rect width=\"100%\" height=\"100%\"{}/>\n{}</svg>\n",
            fill(background),
            self.body
        )
    }
}

pub fn draw(
    svg: &mut Svg,
    primitives: &[Primitive],
    (left, top): (f32, f32),
    (tokens, data): (&Tokens, &DataColors),
) {
    for primitive in primitives {
        match primitive {
            Primitive::Rect {
                x,
                y,
                width,
                height,
                paint,
            } => svg.rect(
                left + x,
                top + y,
                *width,
                *height,
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
            } => svg.text(
                (left + x, top + y),
                content,
                *size,
                Face::from(*font),
                color(*paint, tokens, data),
                *align,
            ),
            Primitive::Chevron {
                x,
                y,
                size,
                forward,
                paint,
            } => {
                let reach = if *forward { size / 2.0 } else { -size / 2.0 };
                let (x, y) = (left + x, top + y);
                let points = [(x - reach, y - size), (x + reach, y), (x - reach, y + size)];
                svg.stroke(&points, color(*paint, tokens, data), 1.0);
            }
            Primitive::Label {
                x,
                y,
                content,
                size,
            } => {
                let width = width(content, *size, Face::Ui);
                let height = (size * CHIP_LINE).round();
                let (x, y) = (left + x, top + y);
                let chip = (
                    x - width - CHIP_PADDING,
                    y - height / 2.0,
                    width + 2.0 * CHIP_PADDING,
                    height,
                );
                svg.rounded(chip, 3.0, tokens.canvas.scale_alpha(CHIP));
                svg.text((x, y), content, *size, Face::Ui, tokens.muted, Align::End);
            }
        }
    }
}

fn number(value: f32) -> String {
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_owned()
}

fn fill(color: Color) -> String {
    let [red, green, blue, _] = color.into_rgba8();
    let mut text = format!(" fill=\"#{red:02x}{green:02x}{blue:02x}\"");
    if color.a < 1.0 {
        let _ = write!(text, " fill-opacity=\"{}\"", number(color.a));
    }
    text
}

fn escape(content: &str) -> String {
    content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use genie_render::scene::{Align, Hue, Paint, Primitive};
    use iced::Color;

    use super::{Face, Svg, draw};
    use crate::theme::{LIGHT, LIGHT_DATA};

    #[test]
    fn shapes_and_text_become_svg_elements() {
        let mut svg = Svg::new(100.0);
        svg.rect(1.0, 2.0, 30.0, 4.5, Color::from_rgb8(0, 114, 178));
        svg.rounded(
            (0.0, 0.0, 10.0, 13.0),
            3.0,
            Color {
                a: 0.85,
                ..Color::WHITE
            },
        );
        svg.text(
            (8.0, 11.0),
            "a<b & \"c\"",
            11.0,
            Face::Ui,
            Color::BLACK,
            Align::Start,
        );
        svg.text(
            (50.0, 5.0),
            "ACGT",
            10.0,
            Face::MonoStrong,
            Color::BLACK,
            Align::Center,
        );
        svg.stroke(&[(0.0, 0.0), (2.0, 3.0), (0.0, 6.0)], Color::BLACK, 1.0);
        let text = svg.finish(40.0, Color::WHITE);
        assert!(
            text.starts_with(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"40\" viewBox=\"0 0 100 40\">"
            ),
            "{text}"
        );
        assert!(
            text.contains("<rect x=\"1\" y=\"2\" width=\"30\" height=\"4.5\" fill=\"#0072b2\"/>"),
            "{text}"
        );
        assert!(text.contains("rx=\"3\""), "{text}");
        assert!(text.contains("fill-opacity=\"0.85\""), "{text}");
        assert!(
            text.contains(">a&lt;b &amp; &quot;c&quot;</text>"),
            "{text}"
        );
        assert!(text.contains("text-anchor=\"middle\""), "{text}");
        assert!(text.contains("font-weight=\"700\""), "{text}");
        assert!(text.contains("Inter"), "{text}");
        assert!(text.contains("JetBrains Mono"), "{text}");
        assert!(text.contains("<polyline points=\"0,0 2,3 0,6\""), "{text}");
        assert!(text.trim_end().ends_with("</svg>"), "{text}");
    }

    #[test]
    fn lines_of_parts_flow_in_one_text() {
        let mut svg = Svg::new(100.0);
        let parts = [
            ("ref.fa", Face::Ui, Color::BLACK),
            ("·", Face::Ui, Color::WHITE),
            ("chr1:1-10", Face::Mono, Color::WHITE),
        ];
        svg.spans((8.0, 11.0), 11.0, &parts, 6.0);
        let text = svg.finish(22.0, Color::WHITE);
        assert_eq!(text.matches("<text ").count(), 1, "{text}");
        assert_eq!(text.matches("<tspan").count(), 3, "{text}");
        assert_eq!(text.matches("<tspan dx=\"6\"").count(), 2, "{text}");
        assert!(text.contains(">chr1:1-10</tspan></text>"), "{text}");
    }

    #[test]
    fn primitives_keep_their_place_and_paint() {
        let mut svg = Svg::new(100.0);
        let primitives = [
            Primitive::Rect {
                x: 1.0,
                y: 2.0,
                width: 3.0,
                height: 4.0,
                paint: Paint::Hue(Hue::Blue),
            },
            Primitive::Chevron {
                x: 20.0,
                y: 5.0,
                size: 2.0,
                forward: true,
                paint: Paint::Hue(Hue::Blue),
            },
            Primitive::Label {
                x: 96.0,
                y: 6.0,
                content: Cow::Borrowed("0-9"),
                size: 10.0,
            },
        ];
        draw(&mut svg, &primitives, (0.0, 10.0), (&LIGHT, &LIGHT_DATA));
        let text = svg.finish(50.0, Color::WHITE);
        assert!(
            text.contains("<rect x=\"1\" y=\"12\" width=\"3\" height=\"4\""),
            "{text}"
        );
        assert!(
            text.contains("<polyline points=\"19,13 21,15 19,17\""),
            "{text}"
        );
        assert!(text.contains(">0-9</text>"), "{text}");
        assert!(text.contains("text-anchor=\"end\""), "{text}");
    }
}
