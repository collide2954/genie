mod app;
mod card;
mod chrome;
mod context;
mod export;
mod font;
mod icon;
mod menu;
mod motion;
mod navigator;
mod paths;
mod recent;
mod reference;
mod scene;
#[cfg(test)]
mod scratch;
mod shortcuts;
mod style;
mod theme;
mod tiles;
mod timer;
mod track;

fn main() -> iced::Result {
    app::run()
}
