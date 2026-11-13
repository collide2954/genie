use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let code = match std::env::current_exe() {
        Ok(helper) => {
            let app = app(&helper);
            run(&app, std::env::args_os().skip(1)).unwrap_or_else(|error| {
                eprintln!("genie-cli cannot start {}. {error}", app.display());
                1
            })
        }
        Err(error) => {
            eprintln!("genie-cli cannot find its folder. {error}");
            1
        }
    };
    std::process::exit(code);
}

fn app(helper: &Path) -> PathBuf {
    helper.with_file_name(format!("genie{}", std::env::consts::EXE_SUFFIX))
}

fn run(app: &Path, arguments: impl IntoIterator<Item = OsString>) -> io::Result<i32> {
    Ok(Command::new(app)
        .args(arguments)
        .status()?
        .code()
        .unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::{app, run};
    use std::ffi::OsString;
    use std::path::Path;

    #[test]
    fn the_app_is_next_to_the_helper() {
        let suffix = std::env::consts::EXE_SUFFIX;
        let helper = Path::new("folder").join(format!("genie-cli{suffix}"));
        assert_eq!(
            app(&helper),
            Path::new("folder").join(format!("genie{suffix}"))
        );
    }

    #[test]
    fn the_helper_gives_the_exit_code_of_the_app() {
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/c")
        } else {
            ("sh", "-c")
        };
        let arguments = [OsString::from(flag), OsString::from("exit 3")];
        assert_eq!(run(Path::new(shell), arguments).unwrap(), 3);
        assert!(run(Path::new("no-such-genie-app"), []).is_err());
    }
}
