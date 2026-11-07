use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use genie_formats::ini::{self, Document, Section};
use genie_render::features::Mode;
use genie_render::scene::Hue;
use genie_render::signal::Style;

use crate::reading::{Frames, Reading};
use crate::track::{Options, Plot};

pub const EXTENSION: &str = "genie";
const MODES: [(&str, Mode); 3] = [
    ("expanded", Mode::Expanded),
    ("squished", Mode::Squished),
    ("collapsed", Mode::Collapsed),
];
const HUES: [(&str, Hue); 5] = [
    ("blue", Hue::Blue),
    ("green", Hue::Green),
    ("orange", Hue::Orange),
    ("vermillion", Hue::Vermillion),
    ("purple", Hue::Purple),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub reference: PathBuf,
    pub locus: Option<String>,
    pub reading: Reading,
    pub tracks: Vec<SavedTrack>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedTrack {
    pub path: PathBuf,
    pub mode: Option<Mode>,
    pub hue: Hue,
    pub height: Option<f32>,
    pub options: Options,
    pub plot: Plot,
}

impl Saved {
    pub fn parse(text: &str, base: &Path) -> Result<Self, String> {
        let document =
            ini::parse(text).map_err(|error| format!("The session file is not valid. {error}"))?;
        let session = document.section("session");
        let value = |key| session.and_then(|section| section.value(key));
        let reference = value("reference")
            .ok_or_else(|| "The session file has no reference sequence.".to_owned())?;
        let reading = Reading {
            reverse: value("strand") == Some("reverse"),
            frames: match value("frames") {
                Some("3") => Frames::Three,
                Some("6") => Frames::Six,
                _ => Frames::Off,
            },
        };
        let tracks = document
            .sections("track")
            .filter_map(|section| {
                let on = |key| section.value(key) == Some("on");
                Some(SavedTrack {
                    path: absolute(section.value("file")?, base),
                    mode: section.value("mode").and_then(|name| find(&MODES, name)),
                    hue: section
                        .value("color")
                        .and_then(|name| find(&HUES, name))
                        .unwrap_or_default(),
                    height: section.value("height").and_then(|text| text.parse().ok()),
                    options: Options {
                        duplicates: on("duplicates"),
                        clips: on("clips"),
                        downsample: section.value("downsample") != Some("off"),
                    },
                    plot: Plot {
                        style: if section.value("style") == Some("line") {
                            Style::Line
                        } else {
                            Style::Bars
                        },
                        linked: section.value("scale") == Some("shared"),
                    },
                })
            })
            .collect();
        Ok(Self {
            reference: absolute(reference, base),
            locus: value("locus").map(str::to_owned),
            reading,
            tracks,
        })
    }

    pub fn text(&self, base: &Path) -> String {
        let mut session = vec![("reference", relative(&self.reference, base))];
        if let Some(locus) = &self.locus {
            session.push(("locus", locus.clone()));
        }
        if self.reading.reverse {
            session.push(("strand", "reverse".to_owned()));
        }
        match self.reading.frames {
            Frames::Off => {}
            Frames::Three => session.push(("frames", "3".to_owned())),
            Frames::Six => session.push(("frames", "6".to_owned())),
        }
        let mut sections = vec![section("session", session)];
        for track in &self.tracks {
            let mut entries = vec![("file", relative(&track.path, base))];
            if let Some(mode) = track.mode {
                entries.push(("mode", name(&MODES, mode).to_owned()));
            }
            entries.push(("color", name(&HUES, track.hue).to_owned()));
            if let Some(height) = track.height {
                entries.push(("height", height.to_string()));
            }
            if track.options.duplicates {
                entries.push(("duplicates", "on".to_owned()));
            }
            if track.options.clips {
                entries.push(("clips", "on".to_owned()));
            }
            if !track.options.downsample {
                entries.push(("downsample", "off".to_owned()));
            }
            if track.plot.style == Style::Line {
                entries.push(("style", "line".to_owned()));
            }
            if track.plot.linked {
                entries.push(("scale", "shared".to_owned()));
            }
            sections.push(section("track", entries));
        }
        ini::write(&Document { sections })
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
        Self::parse(&text, path.parent().unwrap_or(Path::new("")))
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        fs::write(path, self.text(path.parent().unwrap_or(Path::new(""))))
    }
}

pub fn is_session(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

fn section(name: &str, entries: Vec<(&str, String)>) -> Section {
    Section {
        name: name.to_owned(),
        entries: entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    }
}

fn relative(path: &Path, base: &Path) -> String {
    match path.strip_prefix(base) {
        Ok(inside) => inside
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.display().to_string(),
    }
}

fn absolute(text: &str, base: &Path) -> PathBuf {
    let path = PathBuf::from(text);
    if path.is_absolute() || path.has_root() {
        path
    } else {
        base.join(path)
    }
}

fn name<T: Copy + PartialEq>(table: &[(&'static str, T)], value: T) -> &'static str {
    table
        .iter()
        .find(|(_, known)| *known == value)
        .map_or("", |(name, _)| name)
}

fn find<T: Copy>(table: &[(&str, T)], name: &str) -> Option<T> {
    table
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use genie_render::features::Mode;
    use genie_render::scene::Hue;
    use genie_render::signal::Style;

    use super::{Saved, SavedTrack, is_session};
    use crate::reading::{Frames, Reading};
    use crate::track::{Options, Plot};

    fn base() -> PathBuf {
        PathBuf::from("/work/project")
    }

    fn saved() -> Saved {
        Saved {
            reference: base().join("data").join("ref.fa.gz"),
            locus: Some("chr1:51,971-52,030".to_owned()),
            reading: Reading {
                reverse: true,
                frames: Frames::Six,
            },
            tracks: vec![
                SavedTrack {
                    path: base().join("reads.bam"),
                    mode: Some(Mode::Squished),
                    hue: Hue::Blue,
                    height: Some(90.0),
                    options: Options {
                        duplicates: true,
                        clips: true,
                        downsample: false,
                    },
                    plot: Plot::default(),
                },
                SavedTrack {
                    path: PathBuf::from("/elsewhere/genes.bed.gz"),
                    mode: None,
                    hue: Hue::Vermillion,
                    height: None,
                    options: Options::default(),
                    plot: Plot::default(),
                },
                SavedTrack {
                    path: base().join("coverage.bw"),
                    mode: None,
                    hue: Hue::Green,
                    height: Some(64.0),
                    options: Options::default(),
                    plot: Plot {
                        style: Style::Line,
                        linked: true,
                    },
                },
            ],
        }
    }

    #[test]
    fn sessions_read_back_what_they_write() {
        let saved = saved();
        assert_eq!(Saved::parse(&saved.text(&base()), &base()), Ok(saved));
    }

    #[test]
    fn signal_tracks_keep_their_style_and_shared_scale() {
        let text = saved().text(&base());
        assert!(text.contains("style = line"), "{text}");
        assert!(text.contains("scale = shared"), "{text}");
        assert_eq!(text.matches("style =").count(), 1, "{text}");
    }

    #[test]
    fn files_beside_the_session_are_relative() {
        let text = saved().text(&base());
        assert!(text.contains("reference = data/ref.fa.gz"), "{text}");
        assert!(text.contains("file = reads.bam"), "{text}");
        assert!(text.contains("file = /elsewhere/genes.bed.gz"), "{text}");
        let moved = Saved::parse(&text, Path::new("/moved")).unwrap();
        assert_eq!(moved.reference, Path::new("/moved").join("data/ref.fa.gz"));
    }

    #[test]
    fn sessions_need_a_reference() {
        let error = Saved::parse("[session]\nlocus = chr1\n", &base()).unwrap_err();
        assert_eq!(error, "The session file has no reference sequence.");
    }

    #[test]
    fn knows_session_files_by_name() {
        assert!(is_session(Path::new("/a/b.genie")));
        assert!(is_session(Path::new("B.GENIE")));
        assert!(!is_session(Path::new("b.fa")));
    }
}
