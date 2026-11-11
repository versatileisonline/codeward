//! Opt-in macOS host for synthetic control-loop and IPC validation.

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod pilot;

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    if app::run().is_ok() {
        return std::process::ExitCode::SUCCESS;
    }
    eprintln!("Codeward synthetic desktop pilot unavailable");
    std::process::ExitCode::FAILURE
}
