use genie_core::{Locus, Mark, Span, Target, grouped};
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, button, column, container, mouse_area, operation, row, scrollable, space, text,
    text_input,
};
use iced::{Alignment, Border, Element, Length, Task, Theme, mouse};

use crate::chrome;
use crate::icon::{self, Icon};
use crate::motion;
use crate::theme::tokens;
use crate::{font, style};

use super::{Genie, Message, Regions, Session, Sidebar};

pub(super) const SIDEBAR_WIDTH: f32 = 220.0;
const NAME: &str = "region-name";
const CLOSE_SIZE: f32 = 20.0;
const CLOSE_ICON: f32 = 12.0;

impl Genie {
    pub(super) fn regions(&mut self, action: Regions) -> Task<Message> {
        match action {
            Regions::Toggle => self.show_sidebar(self.sidebar.is_none()),
            Regions::Add => {
                if let Some(session) = &mut self.session {
                    let visit = session.visit();
                    let id = session.marks.add(visit.contig, visit.region, None);
                    self.show_sidebar(true);
                    self.sidebar.get_or_insert_default().chosen = Some(id);
                }
            }
            Regions::Choose(id) => {
                let mark = self.mark(id);
                if let Some(mark) = mark {
                    if let Some(sidebar) = &mut self.sidebar {
                        sidebar.chosen = Some(id);
                        sidebar.editing = sidebar.editing.take().filter(|(at, _)| *at == id);
                    }
                    return self.go_to(Target {
                        contig: mark.contig,
                        span: Span::Region(mark.span),
                    });
                }
            }
            Regions::Edit(id) => {
                let mark = self.mark(id);
                if let (Some(sidebar), Some(mark)) = (&mut self.sidebar, mark) {
                    sidebar.chosen = Some(id);
                    sidebar.editing = Some((id, mark.name));
                    return Task::batch([operation::focus(NAME), operation::select_all(NAME)]);
                }
            }
            Regions::Draft(text) => {
                if let Some((_, draft)) = self.editing_mut() {
                    *draft = text;
                }
            }
            Regions::Rename => {
                let editing = self
                    .sidebar
                    .as_mut()
                    .and_then(|sidebar| sidebar.editing.take());
                if let (Some(session), Some((id, name))) = (&mut self.session, editing) {
                    session.marks.rename(id, &name);
                }
            }
            Regions::Remove => {
                if let (Some(session), Some(sidebar)) = (&mut self.session, &mut self.sidebar)
                    && let Some(id) = sidebar.chosen.take()
                {
                    session.marks.remove(id);
                    sidebar.editing = None;
                }
            }
        }
        Task::none()
    }

    fn show_sidebar(&mut self, open: bool) {
        if open == self.sidebar.is_some() {
            return;
        }
        self.sidebar = open.then(Sidebar::default);
        self.now = Instant::now().max(self.now);
        if self.settings.motion {
            self.slide.go_mut(open, self.now);
        } else {
            self.slide = motion::slide(open);
        }
    }

    pub(super) fn sidebar_width(&self) -> f32 {
        self.slide.interpolate(0.0, SIDEBAR_WIDTH, self.now)
    }

    fn mark(&self, id: u64) -> Option<Mark> {
        self.session.as_ref()?.marks.get(id).cloned()
    }

    fn editing_mut(&mut self) -> Option<&mut (u64, String)> {
        self.sidebar.as_mut()?.editing.as_mut()
    }

    pub(super) fn sidebar_view<'a>(&'a self, session: &'a Session) -> Element<'a, Message> {
        let close = button(icon::view(Icon::Close, CLOSE_ICON))
            .width(CLOSE_SIZE)
            .height(CLOSE_SIZE)
            .padding(0)
            .style(style::ghost)
            .on_press(Message::Regions(Regions::Toggle));
        let header = row![
            text("Regions").font(font::UI_STRONG).size(12),
            space::horizontal(),
            close
        ]
        .align_y(Alignment::Center)
        .padding([6, 8]);
        let list: Element<'a, Message> = if session.marks.is_empty() {
            let hint = format!("No regions. Push {} to add the view.", chrome::keys('D'));
            container(text(hint).size(11).style(style::muted))
                .padding([4, 8])
                .height(Length::Fill)
                .into()
        } else {
            let rows = session
                .marks
                .all()
                .iter()
                .map(|mark| self.region_row(session, mark));
            scrollable(Column::with_children(rows).spacing(1).padding([0, 4]))
                .direction(Direction::Vertical(
                    Scrollbar::new().width(6).scroller_width(6).margin(0),
                ))
                .style(style::scrollbar)
                .height(Length::Fill)
                .into()
        };
        let chosen = self.sidebar.as_ref().and_then(|sidebar| sidebar.chosen);
        let action = |label: &'a str, action: Option<Regions>| {
            button(text(label).size(12))
                .padding([3, 8])
                .style(style::ghost)
                .on_press_maybe(action.map(Message::Regions))
        };
        let footer = row![
            action("Add view", Some(Regions::Add)),
            action("Remove", chosen.map(|_| Regions::Remove)),
        ]
        .spacing(4)
        .padding([6, 4]);
        container(column![header, list, footer])
            .width(SIDEBAR_WIDTH)
            .height(Length::Fill)
            .style(|theme| container::background(tokens(theme).bar))
            .into()
    }

    fn region_row<'a>(&self, session: &Session, mark: &'a Mark) -> Element<'a, Message> {
        let locus = Locus {
            name: session.reference.assembly.contigs()[mark.contig]
                .name
                .clone(),
            span: Span::Region(mark.span),
        };
        let size = format!("{} bp", grouped(mark.span.len()));
        let sidebar = self.sidebar.as_ref();
        let chosen = sidebar.and_then(|sidebar| sidebar.chosen) == Some(mark.id);
        let editing = sidebar
            .and_then(|sidebar| sidebar.editing.as_ref())
            .filter(|(id, _)| *id == mark.id);
        let name: Element<'a, Message> = match editing {
            Some((_, draft)) => text_input("Name", draft)
                .id(NAME)
                .size(12)
                .padding([1, 4])
                .style(style::locus)
                .on_input(|text| Message::Regions(Regions::Draft(text)))
                .on_submit(Message::Regions(Regions::Rename))
                .into(),
            None => text(mark.name.as_str()).size(12).width(Length::Fill).into(),
        };
        let first = row![name, text(size).size(11).style(style::muted)]
            .spacing(6)
            .align_y(Alignment::Center);
        let locus = text(locus.to_string())
            .size(11)
            .font(font::MONO)
            .style(style::muted);
        let content = container(column![first, locus].spacing(1))
            .width(Length::Fill)
            .padding([4, 8])
            .style(move |theme| row_style(theme, chosen));
        mouse_area(content)
            .on_press(Message::Regions(Regions::Choose(mark.id)))
            .on_double_click(Message::Regions(Regions::Edit(mark.id)))
            .interaction(mouse::Interaction::Pointer)
            .into()
    }
}

fn row_style(theme: &Theme, chosen: bool) -> container::Style {
    container::Style {
        background: chosen.then(|| tokens(theme).line.into()),
        border: Border::default().rounded(4),
        ..container::Style::default()
    }
}
