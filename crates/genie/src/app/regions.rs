use genie_core::{Locus, Mark, Span, Target, grouped};
use iced::widget::{Column, button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length, Task};

use crate::chrome;
use crate::icon::{self, Icon};
use crate::theme::tokens;
use crate::{font, style};

use super::{Genie, Message, Regions, Session, Sidebar};

pub(super) const SIDEBAR_WIDTH: f32 = 220.0;
const CLOSE_SIZE: f32 = 20.0;
const CLOSE_ICON: f32 = 12.0;

impl Genie {
    pub(super) fn regions(&mut self, action: Regions) -> Task<Message> {
        match action {
            Regions::Toggle => {
                self.sidebar = match self.sidebar.take() {
                    Some(_) => None,
                    None => Some(Sidebar::default()),
                };
            }
            Regions::Add => {
                if let Some(session) = &mut self.session {
                    let visit = session.visit();
                    let id = session.marks.add(visit.contig, visit.region, None);
                    self.sidebar.get_or_insert_default().chosen = Some(id);
                }
            }
            Regions::Choose(id) => {
                let mark = self
                    .session
                    .as_ref()
                    .and_then(|session| session.marks.get(id))
                    .cloned();
                if let Some(mark) = mark {
                    if let Some(sidebar) = &mut self.sidebar {
                        sidebar.chosen = Some(id);
                    }
                    return self.go_to(Target {
                        contig: mark.contig,
                        span: Span::Region(mark.span),
                    });
                }
            }
        }
        Task::none()
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
                .into()
        } else {
            let rows = session
                .marks
                .all()
                .iter()
                .map(|mark| self.region_row(session, mark));
            scrollable(Column::with_children(rows).spacing(1).padding([0, 4]))
                .height(Length::Fill)
                .into()
        };
        let add = button(text("Add view").size(12))
            .padding([3, 8])
            .style(style::ghost)
            .on_press(Message::Regions(Regions::Add));
        let footer = row![add].spacing(4).padding([6, 4]);
        container(column![header, list, space::vertical(), footer])
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
        let detail = format!("{locus} · {} bp", grouped(mark.span.len()));
        let chosen = self.sidebar.as_ref().and_then(|sidebar| sidebar.chosen);
        let style = if chosen == Some(mark.id) {
            style::chosen
        } else {
            style::ghost
        };
        button(
            column![
                text(mark.name.as_str()).size(12),
                text(detail).size(11).font(font::MONO).style(style::muted)
            ]
            .spacing(1),
        )
        .width(Length::Fill)
        .padding([4, 8])
        .style(style)
        .on_press(Message::Regions(Regions::Choose(mark.id)))
        .into()
    }
}
