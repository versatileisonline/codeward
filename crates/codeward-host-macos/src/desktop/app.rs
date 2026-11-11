use std::{cell::RefCell, time::Instant};

use codeward_host_macos::MacosEventSource;
use serde::Serialize;
use tauri::{
    AppHandle, RunEvent, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
    ipc::{InvokeBody, Request},
    webview::NewWindowResponse,
};
use tokio::sync::{oneshot, watch};

use super::pilot::{Pilot, State};

const MAIN_VIEW: &str = "main";
const ASSET: &str = "index.html";

thread_local! {
    static DESKTOP: RefCell<Option<Desktop>> = const { RefCell::new(None) };
}

struct Desktop {
    pilot: Pilot<MacosEventSource>,
    deadline: watch::Sender<Option<Instant>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CommandError {
    Unauthorized,
    InvalidInput,
    Unavailable,
}

fn local_page(url: &Url) -> bool {
    url.scheme() == "tauri"
        && url.host_str() == Some("localhost")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(url.path(), "" | "/" | "/index.html")
        && url.query().is_none()
        && url.fragment().is_none()
}

fn check_command<R: Runtime>(
    window: &WebviewWindow<R>,
    request: &Request<'_>,
) -> Result<(), CommandError> {
    if window.label() != MAIN_VIEW
        || window.as_ref().window().label() != MAIN_VIEW
        || !window.url().is_ok_and(|url| local_page(&url))
    {
        return Err(CommandError::Unauthorized);
    }
    let empty = match request.body() {
        InvokeBody::Json(value) => value.as_object().is_some_and(|fields| fields.is_empty()),
        InvokeBody::Raw(bytes) => bytes.is_empty(),
    };
    if !empty {
        return Err(CommandError::InvalidInput);
    }
    Ok(())
}

fn with_pilot(
    action: impl FnOnce(&mut Pilot<MacosEventSource>) -> State,
) -> Result<State, CommandError> {
    DESKTOP.with(|state| {
        let mut state = state
            .try_borrow_mut()
            .map_err(|_| CommandError::Unavailable)?;
        let desktop = state.as_mut().ok_or(CommandError::Unavailable)?;
        let result = action(&mut desktop.pilot);
        desktop.deadline.send_replace(desktop.pilot.next_tick());
        Ok(result)
    })
}

#[tauri::command]
fn open_synthetic_session<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<State, CommandError> {
    check_command(&window, &request)?;
    with_pilot(|pilot| pilot.open(|| MacosEventSource::for_desktop_loop().ok()))
}

#[tauri::command]
fn get_session_state<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<State, CommandError> {
    check_command(&window, &request)?;
    with_pilot(Pilot::tick)
}

#[tauri::command]
fn lock_vault<R: Runtime>(
    window: WebviewWindow<R>,
    request: Request<'_>,
) -> Result<State, CommandError> {
    check_command(&window, &request)?;
    with_pilot(Pilot::lock)
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
        if app
            .run_on_main_thread(move || {
                let _ = with_pilot(Pilot::tick);
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

pub(super) fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_synthetic_session,
            get_session_state,
            lock_vault
        ])
        .setup(|app| {
            let (deadline, receiver) = watch::channel(None);
            DESKTOP.with(|state| {
                *state.borrow_mut() = Some(Desktop {
                    pilot: Pilot::new(),
                    deadline,
                })
            });
            WebviewWindowBuilder::new(app, MAIN_VIEW, WebviewUrl::App(ASSET.into()))
                .title("Codeward Synthetic Pilot")
                .on_navigation(local_page)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .disable_drag_drop_handler()
                .incognito(true)
                .devtools(false)
                .build()?;
            tauri::async_runtime::spawn(schedule(app.handle().clone(), receiver));
            Ok(())
        })
        .build(context())?;
    app.run(|_, event| match event {
        RunEvent::WindowEvent {
            label,
            event: WindowEvent::Destroyed,
            ..
        } if label == MAIN_VIEW => {
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
        .map(|reply| reply.deserialize().unwrap())
        .map_err(|error| error.as_str().unwrap_or("unexpected error shape").into())
    }

    #[test]
    fn generated_acl_and_command_checks_confine_pilot_ipc() {
        let context = context::<MockRuntime>();
        for asset in [ASSET, "pilot.js"] {
            assert!(context.assets().get(&asset.into()).is_some());
        }
        let app = mock_builder()
            .invoke_handler(tauri::generate_handler![
                open_synthetic_session,
                get_session_state,
                lock_vault
            ])
            .build(context)
            .unwrap();
        let main = WebviewWindowBuilder::new(&app, MAIN_VIEW, WebviewUrl::App(ASSET.into()))
            .build()
            .unwrap();
        let other = WebviewWindowBuilder::new(&app, "other", WebviewUrl::App(ASSET.into()))
            .build()
            .unwrap();
        let (deadline, _receiver) = watch::channel(None);
        DESKTOP.with(|state| {
            *state.borrow_mut() = Some(Desktop {
                pilot: Pilot::new(),
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
        for command in ["open_synthetic_session", "get_session_state", "lock_vault"] {
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
            assert!(local_page(&allowed.parse().unwrap()));
        }
        for denied in [
            "https://localhost/",
            "tauri://localhost.example.test/",
            "tauri://localhost/other.html",
            "tauri://localhost/index.html?state=locked",
            "tauri://localhost/index.html#state",
            "tauri://user@localhost/",
        ] {
            assert!(!local_page(&denied.parse().unwrap()));
        }
    }
}
