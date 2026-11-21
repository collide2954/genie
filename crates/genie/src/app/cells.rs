//! The cell tables of single-cell tracks: for each read or fragment track, a table of the cells
//! with the column that gives the cell types of the barcodes.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::Arc;

use genie_core::samples::Table;
use genie_formats::gzip;
use iced::Task;

use super::{Genie, Message};
use crate::location::Location;
use crate::track::{self, Cells};

/// The largest cell table that genie reads.
const LIMIT: u64 = 256 << 20;

#[derive(Debug, Clone, PartialEq)]
pub enum Celling {
    /// Picks the file of the cell table of a track.
    Pick(u64),
    Picked(u64, Option<PathBuf>),
    /// The cell table that genie read for a track, or why it could not, with the name of the
    /// column of the cell types when a session gives one.
    Opened(u64, Location, Result<Arc<Table>, String>, Option<String>),
    /// Takes the cell types of a column of the cell table of a track.
    Column(u64, usize),
    /// Forgets the cell table of a track.
    Remove(u64),
}

/// Reads a cell table from a file on this computer or on the web.
pub(super) fn read(location: &Location) -> Result<Table, String> {
    let name = location.name();
    if location.size().is_ok_and(|size| size > LIMIT) {
        return Err(format!(
            "The cell table {name} is larger than {} MB.",
            LIMIT >> 20
        ));
    }
    let file = location
        .open()
        .map_err(|error| format!("genie cannot open the cell table {name}. {error}"))?;
    let reader: Box<dyn BufRead> = if track::is_compressed(&name) {
        Box::new(BufReader::new(gzip::Reader::new(BufReader::new(file))))
    } else {
        Box::new(BufReader::new(file))
    };
    genie_formats::samples::read_cells(reader)
        .map_err(|error| format!("The cell table {name} is not valid. {error}"))
}

async fn pick_table() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Open a table of the cells")
        .add_filter("Tables of cells", &["csv", "tsv", "txt", "gz"])
        .pick_file()
        .await
        .map(|file| file.path().to_owned())
}

impl Genie {
    pub(super) fn celling(&mut self, celling: Celling) -> Task<Message> {
        match celling {
            Celling::Pick(id) => {
                self.close_menus();
                return Task::perform(pick_table(), move |path| {
                    Message::Cells(Celling::Picked(id, path))
                });
            }
            Celling::Picked(id, Some(path)) => return self.open_cells(id, path.into(), None),
            Celling::Picked(_, None) => {}
            Celling::Opened(id, location, result, column) => {
                self.busy = self.busy.saturating_sub(1);
                match result {
                    Ok(table) => return self.cells_opened(id, location, table, column.as_deref()),
                    Err(error) => self.fail(error),
                }
            }
            Celling::Column(id, column) => {
                self.close_menus();
                let cells = self.track_cells(id).map(|cells| cells.with_column(column));
                if cells.is_some() {
                    return self.set_cells(id, cells);
                }
            }
            Celling::Remove(id) => {
                self.close_menus();
                return self.set_cells(id, None);
            }
        }
        Task::none()
    }

    /// Reads the cell table of a track in the background, with the name of the column of the
    /// cell types when a session gives one.
    pub(super) fn open_cells(
        &mut self,
        id: u64,
        location: Location,
        column: Option<String>,
    ) -> Task<Message> {
        self.busy += 1;
        Task::perform(
            async move {
                let result = read(&location).map(Arc::new);
                Message::Cells(Celling::Opened(id, location, result, column))
            },
            std::convert::identity,
        )
    }

    fn track_cells(&self, id: u64) -> Option<&Cells> {
        let session = self.session.as_ref()?;
        let track = session.tracks.iter().find(|track| track.id == id)?;
        track.cells.as_ref()
    }

    fn cells_opened(
        &mut self,
        id: u64,
        location: Location,
        table: Arc<Table>,
        column: Option<&str>,
    ) -> Task<Message> {
        let name = location.name();
        let column = column.and_then(|name| table.column(name));
        let Some(cells) = Cells::new(location, table, column) else {
            self.fail(format!(
                "The cell table {name} has no column with 2 to 100 values for cell types."
            ));
            return Task::none();
        };
        let words = format!(
            "The cell table {name} gives {} cell types from {}.",
            cells.map.types.len(),
            cells.column_name()
        );
        let shown = self.inform(words);
        Task::batch([self.set_cells(id, Some(cells)), shown])
    }

    /// Gives a track a cell table, or forgets its cell table, then loads its data again.
    fn set_cells(&mut self, id: u64, cells: Option<Cells>) -> Task<Message> {
        let Some(session) = &mut self.session else {
            return Task::none();
        };
        let Some(track) = session.tracks.iter_mut().find(|track| track.id == id) else {
            return Task::none();
        };
        track.set_cells(cells);
        self.forget(id);
        self.fetch_features()
    }
}
