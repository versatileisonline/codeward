//! Opt-in macOS host for synthetic control-loop and IPC validation.

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod pilot;
#[cfg(target_os = "macos")]
mod probe;

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    {
        let mut args = std::env::args_os().skip(1);
        let scenario = match (args.next(), args.next()) {
            (None, None) => None,
            (Some(flag), None) => match flag.to_str().and_then(probe::Scenario::parse) {
                Some(scenario) => Some(scenario),
                None => {
                    eprintln!("Use --live-lock or --live-sleep");
                    return std::process::ExitCode::FAILURE;
                }
            },
            _ => {
                eprintln!("Use one live probe flag per launch");
                return std::process::ExitCode::FAILURE;
            }
        };
        if app::run(scenario).is_ok() {
            return std::process::ExitCode::SUCCESS;
        }
    }
    eprintln!("Codeward synthetic desktop pilot unavailable");
    std::process::ExitCode::FAILURE
}
