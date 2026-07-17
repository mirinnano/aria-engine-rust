use std::ffi::OsStr;
use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let is_player = arguments
        .first()
        .and_then(|path| std::path::Path::new(path).file_stem())
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.starts_with("aria-player"));
    let result = if is_player {
        #[cfg(all(feature = "desktop-player", not(target_arch = "wasm32")))]
        {
            aria_cli::player::entry(arguments)
        }
        #[cfg(any(not(feature = "desktop-player"), target_arch = "wasm32"))]
        {
            Err(anyhow::anyhow!(
                "this aria-player binary was built without desktop-player support"
            ))
        }
    } else {
        aria_cli::run(arguments)
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(2)
        }
    }
}
