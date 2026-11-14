#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod brand;
mod card;
mod catalog;
mod chrome;
mod context;
mod escape;
mod export;
mod font;
mod genome;
mod grip;
mod help;
mod icon;
mod location;
mod logger;
mod menu;
mod motion;
mod navigator;
mod palette;
mod paths;
mod port;
mod prepare;
mod reading;
mod recent;
mod reference;
mod saved;
mod scene;
#[cfg(test)]
mod scratch;
#[cfg(test)]
#[path = "../../genie-formats/tests/support/server.rs"]
mod server;
mod settings;
mod shortcuts;
mod style;
mod svg;
mod theme;
mod tiles;
mod timer;
mod track;
mod track_dialog;

use std::process::ExitCode;

fn main() -> ExitCode {
    logger::install();
    if let Some(folder) = paths::appimage_folder()
        && let Err(error) = std::env::set_current_dir(&folder)
    {
        log::warn!(
            "genie cannot go to the folder {}. {error}",
            folder.display()
        );
    }
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.first().is_some_and(|first| first == "snapshot") {
        return app::snapshot::command(&arguments[1..]);
    }
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
