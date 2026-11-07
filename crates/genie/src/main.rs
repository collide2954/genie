mod app;
mod chrome;
mod font;
mod icon;
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
mod track;

fn main() -> iced::Result {
    app::run()
}
