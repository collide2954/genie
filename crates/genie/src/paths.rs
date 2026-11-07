use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const APP: &str = "genie";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Cache,
    Config,
}

pub fn cache_dir() -> Option<PathBuf> {
    folder(Kind::Cache, env::consts::OS, |key: &str| env::var_os(key))
}

pub fn config_dir() -> Option<PathBuf> {
    folder(Kind::Config, env::consts::OS, |key: &str| env::var_os(key))
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn folder(kind: Kind, os: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let home = || var("HOME").map(PathBuf::from);
    let base = match (os, kind) {
        ("windows", Kind::Cache) => PathBuf::from(var("LOCALAPPDATA")?),
        ("windows", Kind::Config) => PathBuf::from(var("APPDATA")?),
        ("macos", Kind::Cache) => home()?.join("Library").join("Caches"),
        ("macos", Kind::Config) => home()?.join("Library").join("Application Support"),
        (_, kind) => {
            let (variable, fallback) = match kind {
                Kind::Cache => ("XDG_CACHE_HOME", ".cache"),
                Kind::Config => ("XDG_CONFIG_HOME", ".config"),
            };
            var(variable)
                .map(PathBuf::from)
                .filter(|path| path.has_root())
                .or_else(|| Some(home()?.join(fallback)))?
        }
    };
    Some(base.join(APP))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{Kind, folder};

    fn cache_dir_for(os: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
        folder(Kind::Cache, os, var)
    }

    #[test]
    fn config_follows_each_platform() {
        let windows = folder(
            Kind::Config,
            "windows",
            lookup(&[("APPDATA", "C:/Users/a/AppData/Roaming")]),
        );
        assert_eq!(
            windows,
            Some(PathBuf::from("C:/Users/a/AppData/Roaming").join("genie"))
        );
        let macos = folder(Kind::Config, "macos", lookup(&[("HOME", "/Users/a")]));
        assert_eq!(
            macos,
            Some(PathBuf::from("/Users/a/Library/Application Support/genie"))
        );
        let xdg = folder(
            Kind::Config,
            "linux",
            lookup(&[("XDG_CONFIG_HOME", "/x"), ("HOME", "/home/a")]),
        );
        assert_eq!(xdg, Some(PathBuf::from("/x/genie")));
        let home = folder(Kind::Config, "linux", lookup(&[("HOME", "/home/a")]));
        assert_eq!(home, Some(PathBuf::from("/home/a/.config/genie")));
    }

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        move |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn windows_uses_local_app_data() {
        let path = cache_dir_for(
            "windows",
            lookup(&[("LOCALAPPDATA", "C:/Users/a/AppData/Local")]),
        );
        assert_eq!(
            path,
            Some(PathBuf::from("C:/Users/a/AppData/Local").join("genie"))
        );
    }

    #[test]
    fn macos_uses_library_caches() {
        let path = cache_dir_for("macos", lookup(&[("HOME", "/Users/a")]));
        assert_eq!(path, Some(PathBuf::from("/Users/a/Library/Caches/genie")));
    }

    #[test]
    fn linux_prefers_xdg_cache_home() {
        let path = cache_dir_for(
            "linux",
            lookup(&[("XDG_CACHE_HOME", "/x"), ("HOME", "/home/a")]),
        );
        assert_eq!(path, Some(PathBuf::from("/x/genie")));
        let path = cache_dir_for("linux", lookup(&[("HOME", "/home/a")]));
        assert_eq!(path, Some(PathBuf::from("/home/a/.cache/genie")));
    }

    #[test]
    fn missing_variables_give_no_folder() {
        assert_eq!(cache_dir_for("windows", lookup(&[])), None);
        assert_eq!(cache_dir_for("linux", lookup(&[])), None);
    }
}
