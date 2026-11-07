use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

const APP: &str = "genie";

pub fn cache_dir() -> Option<PathBuf> {
    cache_dir_for(env::consts::OS, |key: &str| env::var_os(key))
}

fn cache_dir_for(os: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let home = || var("HOME").map(PathBuf::from);
    let base = match os {
        "windows" => PathBuf::from(var("LOCALAPPDATA")?),
        "macos" => home()?.join("Library").join("Caches"),
        _ => var("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.has_root())
            .or_else(|| Some(home()?.join(".cache")))?,
    };
    Some(base.join(APP))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::cache_dir_for;

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
