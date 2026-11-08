use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

use genie_core::{Locus, Mark, Region, Span, Target, Viewport, grouped};
use genie_formats::bed;
use genie_formats::features::Entry;
use iced::time::Instant;
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::{
    Column, Stack, button, column, container, mouse_area, operation, pin, row, scrollable, space,
    text, text_input, tooltip,
};
use iced::{Alignment, Border, Element, Length, Task, Theme, mouse};

use crate::chrome;
use crate::context::Context;
use crate::icon::{self, Icon};
use crate::motion;
use crate::paths::{self, file_name};
use crate::theme::tokens;
use crate::{font, style};

use super::{Genie, Message, Regions, Session, Sidebar};

pub(super) const SIDEBAR_WIDTH: f32 = 220.0;
const NAME: &str = "region-name";
const CLOSE_SIZE: f32 = 20.0;
const CLOSE_ICON: f32 = 12.0;
const LIMIT: u64 = 50 << 20;
const BAND_HEIGHT: f32 = 6.0;
const BAND_MIN: f32 = 2.0;

impl Genie {
    pub(super) fn regions(&mut self, action: Regions) -> Task<Message> {
        match action {
            Regions::Toggle => self.show_sidebar(self.sidebar.is_none()),
            Regions::Add => {
                if let Some(visit) = self.session.as_ref().map(Session::visit) {
                    return self.mark_region(visit.contig, visit.region);
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
            Regions::Import => {
                return Task::perform(pick_bed(), |path| {
                    Message::Regions(Regions::ImportPicked(path))
                });
            }
            Regions::ImportPicked(Some(path)) => {
                return Task::perform(async move { read(&path) }, |result| {
                    Message::Regions(Regions::Imported(result))
                });
            }
            Regions::ImportPicked(None) | Regions::ExportPicked(None) => {}
            Regions::Imported(Ok(entries)) => return self.import(entries),
            Regions::Imported(Err(error)) => self.warn(error),
            Regions::Export => {
                return Task::perform(save_bed(), |path| {
                    Message::Regions(Regions::ExportPicked(path))
                });
            }
            Regions::ExportPicked(Some(path)) => return self.export_regions(&with_bed(path)),
        }
        Task::none()
    }

    pub(super) fn mark_region(&mut self, contig: usize, region: Region) -> Task<Message> {
        if let Some(session) = &mut self.session {
            let id = session.marks.add(contig, region, None);
            self.show_sidebar(true);
            self.sidebar.get_or_insert_default().chosen = Some(id);
        }
        Task::none()
    }

    fn import(&mut self, entries: Vec<Entry>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let assembly = &session.reference.assembly;
        let (mut added, mut missing) = (0, 0);
        for entry in entries {
            let found = assembly.find(&entry.contig).map(|contig| {
                let length = assembly.contigs()[contig].length;
                (contig, entry.span.clamp_to(length))
            });
            match found {
                Some((contig, span)) if !span.is_empty() => {
                    session.marks.add(contig, span, Some(entry.name));
                    added += 1;
                }
                _ => missing += 1,
            }
        }
        self.show_sidebar(true);
        let imported = format!("Imported {}.", count(added));
        let text = if missing == 0 {
            imported
        } else {
            let verb = if missing == 1 { "is" } else { "are" };
            let missing = count(missing);
            format!("{imported} {missing} {verb} not on a contig of the reference.")
        };
        self.inform(text)
    }

    fn export_regions(&mut self, path: &Path) -> Task<Message> {
        let Some(session) = &self.session else {
            return Task::none();
        };
        let contigs = session.reference.assembly.contigs();
        let entries: Vec<Entry> = session
            .marks
            .all()
            .iter()
            .map(|mark| Entry {
                name: mark.name.clone(),
                contig: contigs[mark.contig].name.clone(),
                span: mark.span,
            })
            .collect();
        let mut bytes = Vec::new();
        let saved = bed::write(&mut bytes, &entries)
            .and_then(|()| paths::replace(path, &String::from_utf8_lossy(&bytes)));
        match saved {
            Ok(()) => self.inform(format!("Saved {}.", file_name(path))),
            Err(error) => {
                self.warn(format!("The regions could not be saved. {error}"));
                Task::none()
            }
        }
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

    pub(super) fn bands<'a>(&self, session: &'a Session) -> Option<Element<'a, Message>> {
        let viewport = session.viewport;
        let chosen = self.sidebar.as_ref().and_then(|sidebar| sidebar.chosen);
        let bars: Vec<Element<'a, Message>> = session
            .marks
            .on(session.contig)
            .filter_map(|mark| {
                let (x, width) = place(viewport, mark.span)?;
                let picked = chosen == Some(mark.id);
                let bar = container(space())
                    .width(width)
                    .height(BAND_HEIGHT)
                    .style(move |theme| band_style(theme, picked));
                let bar = mouse_area(bar)
                    .on_press(Message::Regions(Regions::Choose(mark.id)))
                    .interaction(mouse::Interaction::Pointer);
                let bar = container(chrome::tip(
                    bar,
                    mark.name.clone(),
                    tooltip::Position::Bottom,
                ))
                .id(format!("band-{}", mark.id));
                Some(pin(bar).x(x).into())
            })
            .collect();
        let any = session.marks.on(session.contig).next().is_some();
        any.then(|| {
            Stack::with_children(bars)
                .width(Length::Fill)
                .height(BAND_HEIGHT)
                .into()
        })
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
        let filled = !session.marks.is_empty();
        let footer = column![
            row![
                action("Add view", Some(Regions::Add)),
                action("Remove", chosen.map(|_| Regions::Remove)),
            ]
            .spacing(4),
            row![
                action("Import BED", Some(Regions::Import)),
                action("Export BED", filled.then_some(Regions::Export)),
            ]
            .spacing(4),
        ]
        .spacing(2)
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
        let id = mark.id;
        let row = mouse_area(content)
            .on_press(Message::Regions(Regions::Choose(id)))
            .on_double_click(Message::Regions(Regions::Edit(id)))
            .interaction(mouse::Interaction::Pointer);
        Context::new(row, move |at| Message::CopyMenu(Some(id), at)).into()
    }
}

fn place(viewport: Viewport, span: Region) -> Option<(f32, f32)> {
    let (left, right) = (
        viewport.to_px(span.start() as f64),
        viewport.to_px(span.end() as f64),
    );
    if right < 0.0 || left > viewport.width() {
        return None;
    }
    let (left, right) = (left.max(0.0), right.min(viewport.width()));
    Some((left as f32, ((right - left) as f32).max(BAND_MIN)))
}

fn band_style(theme: &Theme, chosen: bool) -> container::Style {
    let tokens = tokens(theme);
    container::background(if chosen { tokens.text } else { tokens.muted })
}

pub(super) fn read(path: &Path) -> Result<Vec<Entry>, String> {
    let name = file_name(path);
    let size = fs::metadata(path).map_err(|error| error.to_string())?.len();
    if size > LIMIT {
        return Err(format!("The file {name} is larger than 50 MB."));
    }
    let file = File::open(path).map_err(|error| error.to_string())?;
    bed::regions(BufReader::new(file))
        .map_err(|error| format!("The file {name} is not valid. {error}"))
}

fn with_bed(path: PathBuf) -> PathBuf {
    let is_bed = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("bed"));
    if is_bed {
        return path;
    }
    let mut text = path.into_os_string();
    text.push(".bed");
    PathBuf::from(text)
}

fn count(regions: usize) -> String {
    if regions == 1 {
        "1 region".to_owned()
    } else {
        format!("{regions} regions")
    }
}

async fn pick_bed() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Import regions")
        .add_filter("BED", &["bed", "txt"])
        .pick_file()
        .await
        .map(|file| file.path().to_owned())
}

async fn save_bed() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Export regions")
        .set_file_name("regions.bed")
        .add_filter("BED", &["bed"])
        .save_file()
        .await
        .map(|file| file.path().to_owned())
}

fn row_style(theme: &Theme, chosen: bool) -> container::Style {
    container::Style {
        background: chosen.then(|| tokens(theme).line.into()),
        border: Border::default().rounded(4),
        ..container::Style::default()
    }
}
