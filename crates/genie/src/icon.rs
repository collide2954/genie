use std::f32::consts::TAU;

use iced::widget::canvas::{self, Frame, Geometry, LineCap, LineJoin, Path, Stroke, path::Arc};
use iced::{Element, Length, Point, Radians, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme::tokens;

const GRID: f32 = 24.0;
const STROKE: f32 = 1.75;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sun,
    Moon,
    Monitor,
    Minimize,
    Maximize,
    Close,
    Open,
    Camera,
    Gear,
    Save,
}

enum Shape {
    Line([f32; 2], [f32; 2]),
    Polyline(&'static [[f32; 2]]),
    Rect([f32; 2], [f32; 2], f32),
    Circle([f32; 2], f32),
    Arc([f32; 2], f32, f32, f32),
    Gear([f32; 2], f32, f32, u8),
}

impl Icon {
    fn shapes(self) -> &'static [Shape] {
        match self {
            Self::Sun => &[
                Shape::Circle([12.0, 12.0], 4.0),
                Shape::Line([12.0, 2.0], [12.0, 4.0]),
                Shape::Line([12.0, 20.0], [12.0, 22.0]),
                Shape::Line([2.0, 12.0], [4.0, 12.0]),
                Shape::Line([20.0, 12.0], [22.0, 12.0]),
                Shape::Line([4.93, 4.93], [6.34, 6.34]),
                Shape::Line([17.66, 17.66], [19.07, 19.07]),
                Shape::Line([4.93, 19.07], [6.34, 17.66]),
                Shape::Line([17.66, 6.34], [19.07, 4.93]),
            ],
            Self::Moon => &[
                Shape::Arc([12.0, 12.0], 9.0, 0.0, 270.0),
                Shape::Arc([16.5, 7.5], 6.364, 225.0, 45.0),
            ],
            Self::Monitor => &[
                Shape::Rect([2.0, 3.0], [22.0, 17.0], 2.0),
                Shape::Line([8.0, 21.0], [16.0, 21.0]),
                Shape::Line([12.0, 17.0], [12.0, 21.0]),
            ],
            Self::Minimize => &[Shape::Line([5.0, 12.0], [19.0, 12.0])],
            Self::Maximize => &[Shape::Rect([5.0, 5.0], [19.0, 19.0], 1.5)],
            Self::Close => &[
                Shape::Line([6.0, 6.0], [18.0, 18.0]),
                Shape::Line([18.0, 6.0], [6.0, 18.0]),
            ],
            Self::Open => &[Shape::Polyline(&[
                [3.0, 6.0],
                [9.0, 6.0],
                [11.0, 8.0],
                [21.0, 8.0],
                [21.0, 18.0],
                [3.0, 18.0],
                [3.0, 6.0],
            ])],
            Self::Camera => &[
                Shape::Rect([2.0, 7.0], [22.0, 20.0], 2.0),
                Shape::Polyline(&[[8.0, 7.0], [9.5, 4.0], [14.5, 4.0], [16.0, 7.0]]),
                Shape::Circle([12.0, 13.5], 3.5),
            ],
            Self::Gear => &[
                Shape::Gear([12.0, 12.0], 7.0, 9.5, 8),
                Shape::Circle([12.0, 12.0], 2.75),
            ],
            Self::Save => &[
                Shape::Polyline(&[
                    [4.0, 4.0],
                    [16.5, 4.0],
                    [20.0, 7.5],
                    [20.0, 20.0],
                    [4.0, 20.0],
                    [4.0, 4.0],
                ]),
                Shape::Polyline(&[[8.0, 4.0], [8.0, 9.0], [15.0, 9.0], [15.0, 4.0]]),
                Shape::Polyline(&[[7.5, 20.0], [7.5, 14.0], [16.5, 14.0], [16.5, 20.0]]),
            ],
        }
    }
}

impl Shape {
    fn path(&self, origin: Point, scale: f32) -> Path {
        let point = |[x, y]: [f32; 2]| Point::new(origin.x + x * scale, origin.y + y * scale);
        match *self {
            Self::Line(from, to) => Path::line(point(from), point(to)),
            Self::Polyline(points) => Path::new(|builder| {
                if let Some((first, rest)) = points.split_first() {
                    builder.move_to(point(*first));
                    for next in rest {
                        builder.line_to(point(*next));
                    }
                }
            }),
            Self::Rect(min, max, radius) => Path::rounded_rectangle(
                point(min),
                Size::new((max[0] - min[0]) * scale, (max[1] - min[1]) * scale),
                (radius * scale).into(),
            ),
            Self::Circle(center, radius) => Path::circle(point(center), radius * scale),
            Self::Arc(center, radius, from, to) => Path::new(|builder| {
                builder.arc(Arc {
                    center: point(center),
                    radius: radius * scale,
                    start_angle: Radians(from.to_radians()),
                    end_angle: Radians(to.to_radians()),
                });
            }),
            Self::Gear(center, inner, outer, teeth) => Path::new(|builder| {
                let step = TAU / f32::from(teeth);
                for tooth in 0..teeth {
                    let angle = f32::from(tooth) * step;
                    let corners = [
                        (inner, angle - step * 0.3),
                        (outer, angle - step * 0.17),
                        (outer, angle + step * 0.17),
                        (inner, angle + step * 0.3),
                    ];
                    for (corner, (radius, theta)) in corners.into_iter().enumerate() {
                        let at = point([
                            center[0] + radius * theta.cos(),
                            center[1] + radius * theta.sin(),
                        ]);
                        if tooth == 0 && corner == 0 {
                            builder.move_to(at);
                        } else {
                            builder.line_to(at);
                        }
                    }
                }
                builder.close();
            }),
        }
    }
}

pub fn view<'a, Message: 'a>(icon: Icon, size: f32) -> Element<'a, Message> {
    canvas::Canvas::new(Program { icon, size })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

struct Program {
    icon: Icon,
    size: f32,
}

impl<Message> canvas::Program<Message> for Program {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let tokens = tokens(theme);
        let color = if cursor.is_over(bounds) {
            tokens.text
        } else {
            tokens.muted
        };
        let scale = self.size / GRID;
        let origin = Point::new(
            ((bounds.width - self.size) / 2.0).round(),
            ((bounds.height - self.size) / 2.0).round(),
        );
        let stroke = Stroke::default()
            .with_color(color)
            .with_width(STROKE * scale)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round);
        let mut frame = Frame::new(renderer, bounds.size());
        for shape in self.icon.shapes() {
            frame.stroke(&shape.path(origin, scale), stroke);
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::{Icon, Shape};

    const ICONS: [Icon; 10] = [
        Icon::Save,
        Icon::Sun,
        Icon::Moon,
        Icon::Monitor,
        Icon::Minimize,
        Icon::Maximize,
        Icon::Close,
        Icon::Open,
        Icon::Camera,
        Icon::Gear,
    ];

    fn bounds(shape: &Shape) -> [f32; 4] {
        match *shape {
            Shape::Line([x0, y0], [x1, y1]) => [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)],
            Shape::Polyline(points) => points.iter().fold(
                [f32::MAX, f32::MAX, f32::MIN, f32::MIN],
                |[left, top, right, bottom], [x, y]| {
                    [left.min(*x), top.min(*y), right.max(*x), bottom.max(*y)]
                },
            ),
            Shape::Rect([x0, y0], [x1, y1], _) => [x0, y0, x1, y1],
            Shape::Circle([x, y], radius)
            | Shape::Arc([x, y], radius, _, _)
            | Shape::Gear([x, y], _, radius, _) => [x - radius, y - radius, x + radius, y + radius],
        }
    }

    #[test]
    fn icons_fit_the_grid() {
        for icon in ICONS {
            assert!(!icon.shapes().is_empty(), "{icon:?}");
            for shape in icon.shapes() {
                let [left, top, right, bottom] = bounds(shape);
                assert!(
                    left >= 1.0 && top >= 1.0 && right <= 23.0 && bottom <= 23.0,
                    "{icon:?}"
                );
            }
        }
    }
}
