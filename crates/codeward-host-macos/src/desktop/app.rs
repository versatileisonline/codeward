use std::{cell::RefCell, time::Instant};

use super::setup::{CodeReply, Setup, SetupError, Status as SetupStatus};
use codeward_core::transaction::{StorageError, TransactionError};
use codeward_storage_macos::{ExperimentalStorage, ExperimentalWriterLease};
use serde::Serialize;
use tauri::{
    AppHandle, RunEvent, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
    ipc::{InvokeBody, Request},
    webview::NewWindowResponse,
};
use tokio::sync::{oneshot, watch};

use super::{
    pilot::{Pilot, State},
    probe::{DesktopSource, PROBE_PAGE, Probe, Scenario},
};

const MAIN_VIEW: &str = "main";
const PROBE_VIEW: &str = "session-probe";
const ASSET: &str = "index.html";

thread_local! {
    static DESKTOP: RefCell<Option<Desktop>> = const { RefCell::new(None) };
}

struct Desktop {
    pilot: Pilot<DesktopSource>,
    setup: Setup<ExperimentalWriterLease, DesktopSource>,
    probe: Option<Probe>,
    deadline: watch::Sender<Option<Instant>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CommandError {
    Unauthorized,
    InvalidInput,
    Unavailable,
    Busy,
    Stale,
    InvalidCode,
}

impl From<SetupError> for CommandError {
    fn from(error: SetupError) -> Self {
        match error {
            SetupError::InvalidInput => Self::InvalidInput,
            SetupError::InvalidCode => Self::InvalidCode,
            SetupError::Busy => Self::Busy,
            SetupError::Stale => Self::Stale,
            _ => Self::Unavailable,
        }
    }
}

fn local_page(url: &Url, page: &str) -> bool {
    url.scheme() == "tauri"
        && url.host_str() == Some("localhost")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && (url.path().strip_prefix('/') == Some(page)
            || (page == ASSET && matches!(url.path(), "" | "/")))
        && url.query().is_none()
        && url.fragment().is_none()
}

fn check_origin<R: Runtime>(
    window: &WebviewWindow<R>,
    _request: &Request<'_>,
) -> Result<(), CommandError> {
    let probe = DESKTOP
        .with(|state| {
            state
                .try_borrow()
                .ok()
                .and_then(|state| state.as_ref().map(|desktop| desktop.probe.is_some()))
        })
        .ok_or(CommandError::Unavailable)?;
    let (label, page) = if probe {
        (PROBE_VIEW, PROBE_PAGE)
    } else {
        (MAIN_VIEW, ASSET)
    };
    if window.label() != label
        || window.as_ref().window().label() != label
        || !window.url().is_ok_and(|url| local_page(&url, page))
    {
        return Err(CommandError::Unauthorized);
    }
    Ok(())
}

fn check_command<R: Runtime>(
    window: &WebviewWindow<R>,
    request: &Request<'_>,
) -> Result<(), CommandError> {
    check_origin(window, request)?;
    let empty = match request.body() {
        InvokeBody::Json(value) => value.as_object().is_some_and(|fields| fields.is_empty()),
        InvokeBody::Raw(bytes) => bytes.is_empty(),
    };
    if !empty {
        return Err(CommandError::InvalidInput);
    }
    Ok(())
}

impl Desktop {
    fn reschedule(&self) {
        let deadline = match (
            self.pilot.next_tick(),
            self.probe.as_ref().map(Probe::next_tick),
        ) {
            (Some(pilot), Some(probe)) => Some(pilot.min(probe)),
            (pilot, probe) => pilot.or(probe),
        };
        let deadline = match (deadline, self.setup.next_tick()) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        self.deadline.send_replace(deadline);
    }

    fn tick(&mut self) -> Result<bool, &'static str> {
        self.setup.advance();
        let state = self.pilot.tick();
        let result = self
            .probe
            .as_mut()
            .map_or(Ok(false), |probe| probe.advance(state));
        if !matches!(result, Ok(false)) {
            self.probe.take();
        }
        result
    }
}

fn with_desktop<T>(
    action: impl FnOnce(&mut Desktop) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    DESKTOP.with(|state| {
        let mut state = state
            .try_borrow_mut()
            .map_err(|_| CommandError::Unavailable)?;
        let desktop = state.as_mut().ok_or(CommandError::Unavailable)?;
        let result = action(desktop);
        desktop.reschedule();
        result
    })
}

fn with_setup<T>(
    action: impl FnOnce(&mut Setup<ExperimentalWriterLease, DesktopSource>) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    with_desktop(|desktop| {
        if desktop.probe.is_some() {
            return Err(CommandError::Unauthorized);
        }
        action(&mut desktop.setup)
    })
}

fn epoch(request: &Request<'_>) -> Result<u64, CommandError> {
    let InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::InvalidInput);
    };
    let fields = body.as_object().ok_or(CommandError::InvalidInput)?;
    if fields.len() != 1 {
        return Err(CommandError::InvalidInput);
    }
    fields
        .get("epoch")
        .and_then(|value| value.as_u64())
        .ok_or(CommandError::InvalidInput)
}

fn credential_body<T: serde::de::DeserializeOwned>(
    request: &Request<'_>,
) -> Result<T, CommandError> {
    let InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::InvalidInput);
    };
    T::deserialize(body).map_err(|_| CommandError::InvalidInput)
}
#[tauri::command]
fn list_credentials<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Page, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::ListInput = credential_body(&request)?;
    with_setup(|setup| {
        setup
            .credentials_list(input.epoch, input.revocation, input.cursor.as_deref())
            .map_err(Into::into)
    })
}
#[tauri::command]
fn get_credential_details<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Details, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::Selection = credential_body(&request)?;
    with_setup(|setup| setup.credentials_details(input).map_err(Into::into))
}
#[tauri::command]
fn reveal_credential_password<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Reveal, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::Selection = credential_body(&request)?;
    with_setup(|setup| setup.credentials_reveal(input).map_err(Into::into))
}
#[tauri::command]
fn create_credential<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Operation, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::WriteInput = credential_body(&request)?;
    with_setup(|setup| setup.credentials_write(input, false).map_err(Into::into))
}
#[tauri::command]
fn update_credential<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Operation, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::WriteInput = credential_body(&request)?;
    with_setup(|setup| setup.credentials_write(input, true).map_err(Into::into))
}
#[tauri::command]
fn delete_credential<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Operation, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::Selection = credential_body(&request)?;
    with_setup(|setup| setup.credentials_delete(input).map_err(Into::into))
}
#[tauri::command]
fn get_credential_operation<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<super::setup::credentials::Operation, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::OperationInput = credential_body(&request)?;
    with_setup(|setup| setup.credentials_operation(input).map_err(Into::into))
}

#[tauri::command]
fn set_desktop_dirty<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let input = credential_body(&request)?;
    with_setup(|setup| setup.set_dirty(input).map_err(Into::into))
}
#[tauri::command]
fn resolve_desktop_close<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let input: super::setup::credentials::CloseInput = credential_body(&request)?;
    let status = with_setup(|setup| setup.resolve_close(&input).map_err(Into::into))?;
    if input.allow {
        window.destroy().map_err(|_| CommandError::Unavailable)?;
    }
    Ok(status)
}

#[tauri::command]
fn record_desktop_activity<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let input = credential_body(&request)?;
    with_setup(|setup| setup.record_activity(input).map_err(Into::into))
}

#[tauri::command]
fn open_managed_vault<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_command(&window, &request)?;
    with_setup(|setup| {
        let source = DesktopSource::normal().ok_or(CommandError::Unavailable)?;
        setup
            .open_existing(source, || {
                ExperimentalStorage::managed()
                    .map_err(|_| TransactionError::Storage(StorageError::Unsupported))
            })
            .map_err(Into::into)
    })
}
#[tauri::command]
fn unlock_managed_vault<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::InvalidInput);
    };
    let fields = body.as_object().ok_or(CommandError::InvalidInput)?;
    if fields.len() != 3 {
        return Err(CommandError::InvalidInput);
    }
    let epoch = fields
        .get("epoch")
        .and_then(|v| v.as_u64())
        .ok_or(CommandError::InvalidInput)?;
    let revocation = fields
        .get("revocation")
        .and_then(|v| v.as_u64())
        .ok_or(CommandError::InvalidInput)?;
    let password = fields
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or(CommandError::InvalidInput)?;
    with_setup(|setup| {
        setup
            .unlock(epoch, revocation, password)
            .map_err(Into::into)
    })
}
#[tauri::command]
fn lock_managed_vault<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let epoch = epoch(&request)?;
    with_setup(|setup| setup.lock_existing(epoch).map_err(Into::into))
}

#[tauri::command]
fn begin_vault_setup<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::InvalidInput);
    };
    let fields = body.as_object().ok_or(CommandError::InvalidInput)?;
    if fields.len() != 2 {
        return Err(CommandError::InvalidInput);
    }
    let password = fields
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or(CommandError::InvalidInput)?;
    let confirmation = fields
        .get("confirmation")
        .and_then(|v| v.as_str())
        .ok_or(CommandError::InvalidInput)?;
    with_setup(|setup| {
        let source = DesktopSource::normal().ok_or(CommandError::Unavailable)?;
        setup
            .begin(password, confirmation, source, || {
                ExperimentalStorage::managed()
                    .map_err(|_| TransactionError::Storage(StorageError::Unsupported))
            })
            .map_err(Into::into)
    })
}

#[tauri::command]
fn get_vault_setup_state<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_command(&window, &request)?;
    with_setup(|setup| Ok(setup.status()))
}

#[tauri::command]
fn get_setup_recovery_code<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<CodeReply, CommandError> {
    check_origin(&window, &request)?;
    let epoch = epoch(&request)?;
    with_setup(|setup| setup.code(epoch).map_err(Into::into))
}

#[tauri::command]
fn confirm_vault_setup<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::InvalidInput);
    };
    let fields = body.as_object().ok_or(CommandError::InvalidInput)?;
    if fields.len() != 2 {
        return Err(CommandError::InvalidInput);
    }
    let epoch = fields
        .get("epoch")
        .and_then(|v| v.as_u64())
        .ok_or(CommandError::InvalidInput)?;
    let paper = fields
        .get("paper")
        .and_then(|v| v.as_str())
        .ok_or(CommandError::InvalidInput)?;
    with_setup(|setup| setup.confirm(epoch, paper).map_err(Into::into))
}

#[tauri::command]
fn cancel_vault_setup<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<SetupStatus, CommandError> {
    check_origin(&window, &request)?;
    let epoch = epoch(&request)?;
    with_setup(|setup| setup.cancel(epoch).map_err(Into::into))
}

#[tauri::command]
fn open_synthetic_session<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<State, CommandError> {
    check_command(&window, &request)?;
    with_desktop(|desktop| {
        if desktop.probe.is_some() {
            return Err(CommandError::Unauthorized);
        }
        Ok(desktop.pilot.open(DesktopSource::normal))
    })
}

#[tauri::command]
fn get_session_state<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<State, CommandError> {
    check_command(&window, &request)?;
    // Probe renderer reads must not drive or reschedule the native loop.
    let cached = DESKTOP.with(|state| {
        state.try_borrow().ok().and_then(|state| {
            let desktop = state.as_ref()?;
            desktop.probe.as_ref().map(|_| desktop.pilot.state())
        })
    });
    if let Some(state) = cached {
        return Ok(state);
    }
    with_desktop(|desktop| Ok(desktop.pilot.tick()))
}

#[tauri::command]
fn lock_vault<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<State, CommandError> {
    check_command(&window, &request)?;
    with_desktop(|desktop| {
        if desktop.probe.is_some() {
            return Err(CommandError::Unauthorized);
        }
        let status = desktop.setup.status();
        let _ = desktop.setup.cancel(status.epoch);
        Ok(desktop.pilot.lock())
    })
}

#[tauri::command]
fn probe_visible<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<(), CommandError> {
    check_command(&window, &request)?;
    with_desktop(|desktop| {
        let probe = desktop.probe.as_mut().ok_or(CommandError::Unauthorized)?;
        if probe.arm(desktop.pilot.tick()) {
            Ok(())
        } else {
            Err(CommandError::Unavailable)
        }
    })
}

#[tauri::command]
fn probe_cleared<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<(), CommandError> {
    check_command(&window, &request)?;
    with_desktop(|desktop| {
        let probe = desktop.probe.as_mut().ok_or(CommandError::Unauthorized)?;
        if probe.acknowledge_clear(desktop.pilot.state()) {
            Ok(())
        } else {
            Err(CommandError::Unavailable)
        }
    })
}

async fn schedule(app: AppHandle, mut deadlines: watch::Receiver<Option<Instant>>) {
    loop {
        let deadline = *deadlines.borrow_and_update();
        let Some(deadline) = deadline else {
            if deadlines.changed().await.is_err() {
                return;
            }
            continue;
        };
        match tokio::time::timeout_at(deadline.into(), deadlines.changed()).await {
            Ok(Ok(())) => continue,
            Ok(Err(_)) => return,
            Err(_) => (),
        }
        // Acknowledge the main-thread poll before queueing another wakeup.
        let (finished, polled) = oneshot::channel();
        let main = app.clone();
        if app
            .run_on_main_thread(move || {
                match with_desktop(|desktop| Ok(desktop.tick())) {
                    Ok(Ok(true)) => main.exit(0),
                    Ok(Err(reason)) => {
                        eprintln!("FAIL {reason}");
                        main.exit(1);
                    }
                    _ => (),
                }
                let _ = finished.send(());
            })
            .is_err()
            || polled.await.is_err()
        {
            return;
        }
    }
}

fn context<R: Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

pub(super) fn run(scenario: Option<Scenario>) -> tauri::Result<()> {
    let (label, page) = if scenario.is_some() {
        (PROBE_VIEW, PROBE_PAGE)
    } else {
        (MAIN_VIEW, ASSET)
    };
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_credentials,
            get_credential_details,
            reveal_credential_password,
            create_credential,
            update_credential,
            delete_credential,
            get_credential_operation,
            set_desktop_dirty,
            resolve_desktop_close,
            record_desktop_activity,
            open_managed_vault,
            unlock_managed_vault,
            lock_managed_vault,
            begin_vault_setup,
            get_vault_setup_state,
            get_setup_recovery_code,
            confirm_vault_setup,
            cancel_vault_setup,
            open_synthetic_session,
            get_session_state,
            lock_vault,
            probe_visible,
            probe_cleared
        ])
        .setup(move |app| {
            let (deadline, receiver) = watch::channel(None);
            let probe = scenario
                .map(Probe::new)
                .transpose()
                .map_err(std::io::Error::other)?;
            let mut pilot = Pilot::new();
            if let Some(probe) = &probe {
                pilot.open(|| Some(probe.source()));
            }
            let desktop = Desktop {
                pilot,
                setup: Setup::new(),
                probe,
                deadline,
            };
            desktop.reschedule();
            DESKTOP.with(|state| *state.borrow_mut() = Some(desktop));
            WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
                .title(if scenario.is_some() {
                    "Codeward Session Probe"
                } else {
                    "Codeward"
                })
                .inner_size(1100.0, 760.0)
                .min_inner_size(360.0, 540.0)
                .on_navigation(move |url| local_page(url, page))
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .disable_drag_drop_handler()
                .incognito(true)
                .devtools(false)
                .build()?;
            tauri::async_runtime::spawn(schedule(app.handle().clone(), receiver));
            Ok(())
        })
        .build(context())?;
    app.run(move |app, event| match event {
        RunEvent::ExitRequested {
            code: None, api, ..
        } if scenario.is_some() => {
            api.prevent_exit();
            eprintln!("FAIL probe interrupted before completion");
            app.exit(1);
        }
        RunEvent::WindowEvent {
            label: closing,
            event: WindowEvent::CloseRequested { api, .. },
            ..
        } if closing == label && scenario.is_none() => {
            if with_setup(|setup| Ok(setup.request_close())).unwrap_or(false) {
                api.prevent_close();
            }
        }
        RunEvent::ExitRequested {
            code: None, api, ..
        } if scenario.is_none() => {
            if with_setup(|setup| Ok(setup.request_close())).unwrap_or(false) {
                api.prevent_exit();
            }
        }
        RunEvent::WindowEvent {
            label: closed,
            event: WindowEvent::Destroyed,
            ..
        } if closed == label && scenario.is_none() => {
            DESKTOP.with(|state| {
                state.take();
            });
        }
        RunEvent::Exit => {
            DESKTOP.with(|state| {
                state.take();
            });
        }
        _ => (),
    });
    Ok(())
}

#[cfg(all(test, feature = "desktop-ipc-tests"))]
mod tests {
    use tauri::{
        ipc::CallbackFn,
        test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder},
        webview::InvokeRequest,
    };

    use super::*;

    const LOCAL: &str = "tauri://localhost";

    fn invoke(
        window: &WebviewWindow<MockRuntime>,
        command: &str,
        origin: &str,
        body: Vec<u8>,
    ) -> Result<String, String> {
        get_ipc_response(
            window,
            InvokeRequest {
                cmd: command.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: origin.parse().unwrap(),
                body: InvokeBody::Raw(body),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map(|reply| {
            reply
                .deserialize::<String>()
                .unwrap_or_else(|_| "<structured>".into())
        })
        .map_err(|error| error.as_str().unwrap_or("unexpected error shape").into())
    }

    #[test]
    fn generated_acl_and_command_checks_confine_pilot_ipc() {
        let context = context::<MockRuntime>();
        for asset in [ASSET, "pilot.js", PROBE_PAGE, "probe.js"] {
            assert!(context.assets().get(&asset.into()).is_some());
        }
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                list_credentials,
                get_credential_details,
                reveal_credential_password,
                create_credential,
                update_credential,
                delete_credential,
                get_credential_operation,
                set_desktop_dirty,
                resolve_desktop_close,
                record_desktop_activity,
                open_managed_vault,
                unlock_managed_vault,
                lock_managed_vault,
                begin_vault_setup,
                get_vault_setup_state,
                get_setup_recovery_code,
                confirm_vault_setup,
                cancel_vault_setup,
                open_synthetic_session,
                get_session_state,
                lock_vault,
                probe_visible,
                probe_cleared
            ])
            .build(context)
            .unwrap();
        let main = WebviewWindowBuilder::new(&app, MAIN_VIEW, WebviewUrl::App(ASSET.into()))
            .build()
            .unwrap();
        let other = WebviewWindowBuilder::new(&app, "other", WebviewUrl::App(ASSET.into()))
            .build()
            .unwrap();
        let probe = WebviewWindowBuilder::new(&app, PROBE_VIEW, WebviewUrl::App(PROBE_PAGE.into()))
            .build()
            .unwrap();
        let (deadline, _receiver) = watch::channel(None);
        DESKTOP.with(|state| {
            *state.borrow_mut() = Some(Desktop {
                pilot: Pilot::new(),
                setup: Setup::new(),
                probe: None,
                deadline,
            })
        });

        assert_eq!(
            invoke(&main, "get_session_state", LOCAL, vec![]),
            Ok("closed".into())
        );
        assert_eq!(
            invoke(&main, "lock_vault", LOCAL, vec![]),
            Ok("closed".into())
        );
        assert_eq!(
            invoke(&main, "lock_vault", LOCAL, vec![0]),
            Err("invalid_input".into())
        );
        for command in [
            "open_synthetic_session",
            "get_session_state",
            "lock_vault",
            "open_managed_vault",
            "unlock_managed_vault",
            "lock_managed_vault",
            "begin_vault_setup",
            "get_vault_setup_state",
            "get_setup_recovery_code",
            "confirm_vault_setup",
            "cancel_vault_setup",
            "list_credentials",
            "get_credential_details",
            "reveal_credential_password",
            "create_credential",
            "update_credential",
            "delete_credential",
            "get_credential_operation",
            "set_desktop_dirty",
            "resolve_desktop_close",
            "record_desktop_activity",
        ] {
            assert!(
                invoke(&other, command, LOCAL, vec![])
                    .unwrap_err()
                    .contains("not allowed")
            );
            assert!(
                invoke(&main, command, "https://example.test", vec![])
                    .unwrap_err()
                    .contains("not allowed")
            );
            assert!(
                invoke(&main, command, "tauri://localhost.example.test", vec![])
                    .unwrap_err()
                    .contains("not allowed")
            );
        }
        for command in ["probe_visible", "probe_cleared"] {
            assert!(
                invoke(&main, command, LOCAL, vec![])
                    .unwrap_err()
                    .contains("not allowed")
            );
        }
        for command in [
            "open_synthetic_session",
            "lock_vault",
            "open_managed_vault",
            "unlock_managed_vault",
            "lock_managed_vault",
            "begin_vault_setup",
            "get_vault_setup_state",
            "get_setup_recovery_code",
            "confirm_vault_setup",
            "cancel_vault_setup",
            "list_credentials",
            "get_credential_details",
            "reveal_credential_password",
            "create_credential",
            "update_credential",
            "delete_credential",
            "get_credential_operation",
            "set_desktop_dirty",
            "resolve_desktop_close",
            "record_desktop_activity",
        ] {
            assert!(
                invoke(&probe, command, LOCAL, vec![])
                    .unwrap_err()
                    .contains("not allowed")
            );
        }
        assert_eq!(
            invoke(&probe, "get_session_state", LOCAL, vec![]),
            Err("unauthorized".into())
        );
        assert_eq!(
            invoke(&main, "get_vault_setup_state", LOCAL, vec![]),
            Ok("<structured>".into())
        );
        for command in [
            "unlock_managed_vault",
            "lock_managed_vault",
            "begin_vault_setup",
            "get_setup_recovery_code",
            "confirm_vault_setup",
            "cancel_vault_setup",
            "list_credentials",
            "get_credential_details",
            "reveal_credential_password",
            "create_credential",
            "update_credential",
            "delete_credential",
            "get_credential_operation",
            "set_desktop_dirty",
            "resolve_desktop_close",
            "record_desktop_activity",
        ] {
            assert_eq!(
                invoke(&main, command, LOCAL, vec![]),
                Err("invalid_input".into())
            );
        }
        assert!(invoke(&main, "plugin:app|version", LOCAL, vec![]).is_err());
        assert!(invoke(&main, "save_credential", LOCAL, vec![]).is_err());
        assert!(invoke(&main, "open_synthetic_session", LOCAL, vec![]).is_ok());
        main.navigate("tauri://localhost/other.html".parse().unwrap())
            .unwrap();
        assert_eq!(
            invoke(&main, "get_session_state", LOCAL, vec![]),
            Err("unauthorized".into())
        );
        DESKTOP.with(|state| {
            state.take();
        });
    }

    #[test]
    fn runtime_navigation_policy_allows_only_the_bundled_document() {
        for allowed in [
            "tauri://localhost",
            "tauri://localhost/",
            "tauri://localhost/index.html",
        ] {
            assert!(local_page(&allowed.parse().unwrap(), ASSET));
        }
        assert!(local_page(
            &"tauri://localhost/probe.html".parse().unwrap(),
            PROBE_PAGE
        ));
        assert!(!local_page(
            &"tauri://localhost/probe.html".parse().unwrap(),
            ASSET
        ));
        for denied in [
            "https://localhost/",
            "tauri://localhost.example.test/",
            "tauri://localhost/other.html",
            "tauri://localhost/index.html?state=locked",
            "tauri://localhost/index.html#state",
            "tauri://user@localhost/",
        ] {
            assert!(!local_page(&denied.parse().unwrap(), ASSET));
        }
    }
}
