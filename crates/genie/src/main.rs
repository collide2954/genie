mod app;
mod card;
mod chrome;
mod context;
mod export;
mod font;
mod genome;
mod grip;
mod icon;
mod location;
mod logger;
mod menu;
mod motion;
mod navigator;
mod paths;
mod port;
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
mod theme;
mod tiles;
mod timer;
mod track;

use std::process::ExitCode;

fn main() -> ExitCode {
    logger::install();
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
