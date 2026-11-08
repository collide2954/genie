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

fn main() -> iced::Result {
    logger::install();
    app::run()
}
