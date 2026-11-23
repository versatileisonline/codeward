import { useEffect, useRef, useState, type FormEvent } from "react";
import { invoke, type Status } from "./native";
import { Vault } from "./Vault";
import { Welcome } from "./Welcome";
import { MenuIcon, ThemeIcon } from "./SidebarIcons";
import { PasswordInput } from "./PasswordInput";
// Screens share one revocable native session; no browser persistence owns vault state.
export function App() {
  const [view, setView] = useState("home"),
    [dark, setDark] = useState(true),
    [collapsed, setCollapsed] = useState(true),
    [status, setStatus] = useState<Status | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [message, setMessage] = useState(""),
    [code, setCode] = useState(""),
    [transcribing, setTranscribing] = useState(false);
  const guard = useRef<((action: () => void) => void) | null>(null);
  function navigate(next = "home") {
    if (guard.current) guard.current(() => leave(next));
    else leave(next);
  }
  const seq = useRef(0),
    current = useRef<Status | null>(null),
    poll = useRef(false),
    pending = useRef(false),
    mode = useRef(false),
    alive = useRef(true),
    recoveryEntry = useRef<"code" | "recheck" | null>(null);
  function clear() {
    recoveryEntry.current = null;
    setCode("");
    setError("");
    setMessage("");
    setBusy(false);
    pending.current = false;
    setTranscribing(false);
  }
  function leave(next = "home") {
    const previous = current.current;
    seq.current++;
    current.current = null;
    setStatus(null);
    clear();
    setView(next);
    if (previous)
      void invoke("cancel_vault_setup", { epoch: previous.epoch }).catch(
        () => {},
      );
  }
  function apply(s: Status, token: number) {
    if (
      token !== seq.current ||
      s.epoch !== current.current?.epoch ||
      s.revocation < current.current.revocation
    )
      return;
    const revoked = s.revocation !== current.current.revocation;
    current.current = s;
    setStatus(s);
    if (revoked) {
      setCode("");
      setTranscribing(false);
      recoveryEntry.current = null;
      setError("");
    }
    if (mode.current && s.state.startsWith("recovery_")) {
      if (s.state === "recovery_unknown" && s.error === "unavailable")
        recoveryEntry.current = null;
      if (s.state === "recovery_ready") {
        recoveryEntry.current = null;
        setView("recover-password");
      } else if (s.state === "recovery_failed" && recoveryEntry.current === "code")
        setView("recover-code");
      else if (s.state === "recovery_unknown" && recoveryEntry.current === "recheck")
        setView("recover-recheck");
      else if (s.state === "recovery_checking") setView("recover-recheck");
      else setView("recover-result");
      if (s.error === "invalid_code")
        setError("The code doesn't match this vault. Try again.");
      return;
    }
    if (mode.current && s.state === "recovering") {
      setView("recover-code");
      return;
    }
    if (mode.current && s.state === "locked" && !revoked && recoveryEntry.current === "code") {
      setView("recover-code");
      if (s.error === "invalid_code")
        setError("The code doesn't match this vault. Try again.");
      return;
    }
    if (mode.current && ["locked", "unlocking", "unlocked"].includes(s.state)) {
      setView(s.state === "unlocked" ? "vault" : "unlock");
      setError(
        s.error === "wrong_password"
          ? "Incorrect password. Try again."
          : s.error
            ? "Couldn't unlock. Try again."
            : "",
      );
      return;
    }
    if (["recovery", "saving"].includes(s.state)) {
      setView("recovery");
      if (s.error === "invalid_code")
        setError("The code doesn't match. Try again.");
      return;
    }
    if (s.state !== "opening") {
      setCode("");
      setView("result");
    }
  }
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }, [dark]);
  useEffect(() => {
    document.body.classList.toggle("sidebar-collapsed", collapsed);
    return () => document.body.classList.remove("sidebar-collapsed");
  }, [collapsed]);
  useEffect(() => {
    document.querySelector<HTMLElement>("main h2, main input")?.focus();
  }, [view]);
  useEffect(() => {
    alive.current = true;
    const timer = window.setInterval(async () => {
      if (!current.current || poll.current || pending.current) return;
      poll.current = true;
      const token = seq.current;
      try {
        apply(await invoke<Status>("get_vault_setup_state"), token);
      } catch {
        if (token === seq.current) {
          leave("connection");
        }
      } finally {
        poll.current = false;
      }
    }, 250);
    const hide = () => {
      if (document.visibilityState === "hidden") leave();
    };
    const page = () => leave();
    document.addEventListener("visibilitychange", hide);
    window.addEventListener("pagehide", page);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", hide);
      window.removeEventListener("pagehide", page);
    };
  }, []);
  useEffect(() => {
    if (
      view !== "recovery" ||
      status?.state !== "recovery" ||
      transcribing ||
      code
    )
      return;
    let valid = true;
    const token = seq.current,
      epoch = status.epoch;
    void invoke<{ epoch: number; code: string }>("get_setup_recovery_code", {
      epoch,
    })
      .then((r) => {
        if (
          valid &&
          token === seq.current &&
          current.current?.state === "recovery" &&
          r.epoch === epoch
        )
          setCode(r.code);
      })
      .catch(() => {
        if (valid && token === seq.current)
          setError("Code unavailable. Cancel setup and start again.");
      });
    return () => {
      valid = false;
    };
  }, [view, status?.state, status?.epoch, transcribing, code]);
  async function open() {
    leave("unlock");
    mode.current = true;
    setBusy(true);
    setMessage("Opening vault…");
    pending.current = true;
    const token = seq.current;
    try {
      const s = await invoke<Status>("open_managed_vault");
      if (token !== seq.current) {
        void invoke("cancel_vault_setup", { epoch: s.epoch }).catch(() => {});
        return;
      }
      current.current = s;
      apply(s, token);
    } catch (e) {
      if (token === seq.current)
        setError(
          e === "desktop_required"
            ? "Open the desktop app to unlock your vault."
            : "Vault unavailable. Reopen Codeward and try again.",
        );
    } finally {
      if (token === seq.current) {
        setBusy(false);
        setMessage("");
        pending.current = false;
      }
    }
  }
  async function begin(password: string, confirmation: string) {
    if (pending.current) return;
    if (!password) {
      setError("Enter a master password.");
      return;
    }
    if (new TextEncoder().encode(password).length > 4096) {
      setError("Use a password of 4,096 UTF-8 bytes or fewer.");
      return;
    }
    if (password !== confirmation) {
      setError("Your passwords do not match.");
      return;
    }
    mode.current = false;
    const token = ++seq.current;
    pending.current = true;
    setBusy(true);
    setError("");
    setMessage("Preparing recovery code…");
    try {
      const s = await invoke<Status>("begin_vault_setup", {
        password,
        confirmation,
      });
      if (token !== seq.current) {
        void invoke("cancel_vault_setup", { epoch: s.epoch }).catch(() => {});
        return;
      }
      current.current = s;
      apply(s, token);
    } catch (e) {
      if (token === seq.current)
        setError(
          e === "desktop_required"
            ? "Open the desktop app to create a vault."
            : "Setup couldn't start. Reopen Codeward and try again.",
        );
    } finally {
      if (token === seq.current) {
        setBusy(false);
        setMessage("");
        pending.current = false;
      }
    }
  }
  async function unlock(password: string) {
    if (!current.current || pending.current) return;
    if (!password || new TextEncoder().encode(password).length > 4096) {
      setError("Enter a password of 1–4,096 UTF-8 bytes.");
      return;
    }
    const token = ++seq.current;
    pending.current = true;
    setBusy(true);
    setError("");
    const s = current.current;
    try {
      apply(
        await invoke<Status>("unlock_managed_vault", {
          epoch: s.epoch,
          revocation: s.revocation,
          password,
        }),
        token,
      );
    } catch {
      if (token === seq.current)
        setError("Couldn't confirm unlocking. Checking vault status…");
    } finally {
      if (token === seq.current) {
        setBusy(false);
        pending.current = false;
      }
    }
  }
  function enterRecovery(recheck = false) {
    if (pending.current) return;
    ++seq.current;
    setError("");
    setMessage("");
    recoveryEntry.current = recheck ? "recheck" : "code";
    setView(recheck ? "recover-recheck" : "recover-code");
  }
  async function recoverPaper(paper: string) {
    if (!current.current || pending.current) return;
    if (!paper.trim()) {
      setError("Enter your recovery code.");
      return;
    }
    if (new TextEncoder().encode(paper).length > 256) {
      setError("The code doesn't match this vault. Try again.");
      return;
    }
    const recheck = recoveryEntry.current === "recheck";
    const s = current.current;
    const token = ++seq.current;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      apply(await invoke<Status>(recheck ? "reconcile_vault_recovery" : "begin_vault_recovery", {
        epoch: s.epoch, revocation: s.revocation, paper,
      }), token);
    } catch {
      if (token === seq.current)
        setError("Couldn't confirm the code. Checking vault status…");
    } finally {
      if (token === seq.current) {
        pending.current = false;
        setBusy(false);
      }
    }
  }
  async function saveRecoveredPassword(password: string, confirmation: string) {
    if (!current.current || pending.current) return;
    if (!password) {
      setError("Enter a master password.");
      return;
    }
    if (new TextEncoder().encode(password).length > 4096) {
      setError("Use a password of 4,096 UTF-8 bytes or fewer.");
      return;
    }
    if (password !== confirmation) {
      setError("Your passwords do not match.");
      return;
    }
    const s = current.current;
    const token = ++seq.current;
    recoveryEntry.current = null;
    pending.current = true;
    setBusy(true);
    setError("");
    // Drop the password form before a write can enter an uncertain state.
    setView("recover-result");
    setMessage("Saving new password…");
    try {
      apply(await invoke<Status>("complete_vault_recovery", {
        epoch: s.epoch, revocation: s.revocation, password, confirmation,
      }), token);
    } catch {
      if (token === seq.current)
        setMessage("Checking whether your new password was saved…");
    } finally {
      if (token === seq.current) {
        pending.current = false;
        setBusy(false);
      }
    }
  }
  async function backToUnlock() {
    recoveryEntry.current = null;
    setError("");
    setMessage("");
    setCode("");
    pending.current = false;
    await lock();
  }
  async function confirm(paper: string) {
    if (!current.current || pending.current) return;
    if (!paper) {
      setError("Enter your recovery code.");
      return;
    }
    const token = ++seq.current;
    pending.current = true;
    setBusy(true);
    setError("");
    try {
      apply(
        await invoke<Status>("confirm_vault_setup", {
          epoch: current.current.epoch,
          paper,
        }),
        token,
      );
    } catch (e) {
      if (token === seq.current)
        setError(
          e === "invalid_code"
            ? "The code doesn't match. Try again."
            : "Checking vault status…",
        );
    } finally {
      if (token === seq.current) {
        setBusy(false);
        pending.current = false;
      }
    }
  }
  async function lock() {
    const s = current.current;
    if (!s) return;
    const token = ++seq.current;
    pending.current = true;
    setView("unlock");
    setBusy(true);
    try {
      apply(
        await invoke<Status>("lock_managed_vault", { epoch: s.epoch }),
        token,
      );
    } catch {
      if (token === seq.current) leave("connection");
    } finally {
      if (token === seq.current) {
        pending.current = false;
        setBusy(false);
      }
    }
  }
  const result: Record<string, [string, string]> = {
    created: ["Vault saved", "Your vault is saved and locked."],
    existing: [
      "Vault already exists",
      "Your existing vault was left unchanged.",
    ],
    locked: ["Setup locked", "Start again to continue."],
    unknown: [
      "Save not confirmed",
      "Keep your recovery code. Quit and reopen Codeward to check the vault before trying again.",
    ],
    failed: [
      mode.current ? "Vault couldn't be opened" : "Setup couldn't finish",
      "No successful save was confirmed. The file was left unchanged. Reopen Codeward and try again.",
    ],
    missing: ["No vault found", "Create a vault to get started."],
  };
  return (
    <>
      <aside className="sidebar" aria-label="Sidebar">
        <div className="sidebar-top">
          <button
            id="sidebar-toggle"
            className="sidebar-toggle"
            aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
            aria-expanded={!collapsed}
            aria-controls="navigation"
            onClick={() => setCollapsed(!collapsed)}
          >
            <MenuIcon />
          </button>
        </div>
        <nav id="navigation" aria-label="Main navigation">
          <button
            className="nav-button"
            aria-current={view !== "about" ? "page" : undefined}
            onClick={() => navigate()}
          >
            <span className="nav-icon" aria-hidden="true">
              ⌂
            </span>
            <span className="nav-label">Home</span>
          </button>
          <button
            className="nav-button"
            aria-current={view === "about" ? "page" : undefined}
            onClick={() => navigate("about")}
          >
            <span className="nav-icon" aria-hidden="true">
              ⓘ
            </span>
            <span className="nav-label">About</span>
          </button>
        </nav>
        <div className="sidebar-bottom">
          <p className="author">Creator: Bryan Torres</p>
          <button
            className="theme-toggle"
            aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
            aria-pressed={dark}
            onClick={() => setDark(!dark)}
          >
            <span className="theme-icon">
              <ThemeIcon dark={dark} />
            </span>
            <span className="theme-label">{dark ? "Light" : "Dark"} mode</span>
          </button>
        </div>
      </aside>
      <main>
        {view === "home" && (
          <Welcome create={() => leave("setup")} open={open} />
        )}
        {view === "setup" && (
          <section className="setup-panel">
            <Back onClick={() => leave()} />
            <p className="step-label">Step 01 / Vault password</p>
            <h2 tabIndex={-1}>Create your vault</h2>
            <PasswordForm
              key={`setup-${status?.revocation}`}
              setup
              busy={busy}
              error={error}
              message={message}
              submit={begin}
            />
            <p className="preview-note">
              Development build. Use sample passwords only.
            </p>
          </section>
        )}
        {view === "unlock" && (
          <section className="setup-panel">
            <Back onClick={() => leave()} />
            <h2 tabIndex={-1}>Unlock your vault</h2>
            <PasswordForm
              key={`unlock-${status?.revocation}`}
              busy={
                busy ||
                status?.state === "opening" ||
                status?.state === "unlocking"
              }
              error={error}
              message={
                message || (status?.state === "unlocking" ? "Unlocking…" : "")
              }
              submit={unlock}
            />
            <button className="back-button view-code" disabled={busy || status?.state !== "locked"} onClick={() => enterRecovery()}>
              Forgot password?
            </button>
          </section>
        )}
        {(view === "recover-code" || view === "recover-recheck") && (
          <section className="setup-panel">
            {view === "recover-code" ? (
              <Back label="Back to unlock" onClick={() => { void backToUnlock(); }} />
            ) : (
              <Back label="Back" disabled={busy || status?.state === "recovery_checking"} onClick={() => {
                ++seq.current;
                recoveryEntry.current = null;
                setError("");
                setView("recover-result");
              }} />
            )}
            <p className="step-label">Step 01 / Recovery code</p>
            <h2 tabIndex={-1}>{view === "recover-recheck" ? "Check saved state" : "Recover your vault"}</h2>
            <p className="recovery-hint">
              {view === "recover-recheck"
                ? "Enter your recovery code again to check whether your new password was saved."
                : "Enter the recovery code for this vault to choose a new master password."}
            </p>
            <p className="field-hint">Your code must match this intact vault. It cannot restore a lost vault file.</p>
            <RecoveryCodeForm
              key={`${view}-${status?.revocation}`}
              busy={busy || status?.state === "recovering" || status?.state === "recovery_checking"}
              error={error}
              submit={recoverPaper}
            />
          </section>
        )}
        {view === "recover-password" && (
          <section className="setup-panel">
            <Back label="Back to unlock" onClick={() => { void backToUnlock(); }} />
            <p className="step-label">Step 02 / New vault password</p>
            <h2 tabIndex={-1}>Choose a new password</h2>
            <p className="recovery-hint">Your existing recovery code will still work.</p>
            <PasswordForm
              key={`recover-password-${status?.revocation}`}
              setup recovery busy={busy} error={error} message={message}
              submit={saveRecoveredPassword}
            />
          </section>
        )}
        {view === "recover-result" && (
          <section className="setup-panel">
            <h2 tabIndex={-1}>
              {status?.state === "recovery_saved" ? "New password saved"
                : status?.state === "recovery_unknown" ? "Save not confirmed"
                  : status?.state === "recovery_failed" ? "Password wasn't saved"
                    : "Saving new password…"}
            </h2>
            <p className="recovery-hint" role="status">
              {status?.state === "recovery_saved"
                ? "Your new master password is saved. Your existing recovery code still works."
                : status?.state === "recovery_unknown"
                  ? status.error === "unavailable"
                    ? "Codeward could not check the saved state. Keep your recovery code, quit and reopen Codeward, then use recovery again if needed."
                    : "Keep your recovery code. Check the saved state before trying to unlock or save another password."
                  : status?.state === "recovery_failed"
                    ? "Your previous vault state is unchanged. Enter your recovery code again to retry."
                    : "Keep your recovery code while Codeward confirms the saved state."}
            </p>
            <p className="form-error" role="alert">{error}</p>
            <div className="result-actions">
              {status?.state === "recovery_unknown" && status.error !== "unavailable" && (
                <button className="primary-button" disabled={busy} onClick={() => enterRecovery(true)}>Check saved state</button>
              )}
              {status?.state === "recovery_failed" && (
                <button className="primary-button" disabled={busy} onClick={() => enterRecovery()}>Try recovery again</button>
              )}
              {["recovery_saved", "recovery_failed"].includes(status?.state ?? "") && (
                <button className="secondary-button" disabled={busy} onClick={() => { void backToUnlock(); }}>Back to unlock</button>
              )}
            </div>
          </section>
        )}
        {view === "recovery" && (
          <section className="setup-panel">
            <Back label="Cancel setup" onClick={() => leave()} />
            <p className="step-label">Step 02 / Recovery code</p>
            <h2 tabIndex={-1}>Keep a way back in</h2>
            {!transcribing ? (
              <>
                <p className="recovery-hint">
                  Write down this code. Keep it somewhere safe.
                </p>
                <p className="recovery-code" aria-label="Recovery code">
                  {code}
                </p>
                <p className="field-hint">
                  It restores access to an intact vault if you forget your
                  password. It cannot restore a lost vault file.
                </p>
                <button
                  className="primary-button continue-button"
                  disabled={!code}
                  onClick={() => {
                    setCode("");
                    setTranscribing(true);
                    setError("");
                  }}
                >
                  I've written it down
                </button>
              </>
            ) : (
              <RecoveryForm
                busy={busy || status?.state === "saving"}
                error={error}
                submit={confirm}
                view={() => {
                  setTranscribing(false);
                  setError("");
                }}
              />
            )}
            <p role="status" className="setup-status">
              {status?.state === "saving" ? "Saving vault…" : ""}
            </p>
          </section>
        )}
        {view === "result" && (
          <section className="setup-panel">
            <h2 tabIndex={-1}>{result[status?.state ?? ""]?.[0]}</h2>
            <p className="recovery-hint">{result[status?.state ?? ""]?.[1]}</p>
            <div className="result-actions">
              {["created", "existing"].includes(status?.state ?? "") && (
                <button className="primary-button" onClick={open}>
                  Unlock Vault
                </button>
              )}
              <button
                className="secondary-button home-back-button"
                onClick={() => leave()}
              >
                <span aria-hidden="true">←</span> Back to Home
              </button>
            </div>
          </section>
        )}
        {view === "connection" && (
          <section className="setup-panel">
            <h2 tabIndex={-1}>Connection lost</h2>
            <p className="recovery-hint">
              Keep your recovery code. Reopen Codeward to check the vault.
            </p>
            <button
              className="secondary-button home-back-button"
              onClick={() => leave()}
            >
              <span aria-hidden="true">←</span> Back to Home
            </button>
          </section>
        )}
        {view === "vault" && status && (
          <Vault
            key={`${status.epoch}-${status.revocation}`}
            status={status}
            lock={lock}
            guard={guard}
          />
        )}
        {view === "about" && (
          <section className="about-panel">
            <h2 tabIndex={-1}>About Codeward</h2>
            <p>Your local password manager.</p>
            <button
              className="back-button home-back-button"
              onClick={() => leave()}
            >
              <span aria-hidden="true">←</span> Back to Home
            </button>
          </section>
        )}
      </main>
    </>
  );
}
function Back({
  onClick,
  label = "Back",
  disabled = false,
}: {
  onClick: () => void;
  label?: string;
  disabled?: boolean;
}) {
  return (
    <button className="back-button" disabled={disabled} onClick={onClick}>
      <span aria-hidden="true">←</span>
      {label}
    </button>
  );
}
function PasswordForm({
  setup = false,
  recovery = false,
  busy,
  error,
  message,
  submit,
}: {
  setup?: boolean;
  recovery?: boolean;
  busy: boolean;
  error: string;
  message: string;
  submit: (p: string, c: string) => Promise<void>;
}) {
  const [password, setPassword] = useState(""),
    [confirmation, setConfirmation] = useState("");
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (busy) return;
        void submit(password, confirmation);
        setPassword("");
        setConfirmation("");
      }}
      noValidate
    >
      <PasswordInput
        label={recovery ? "New master password" : "Master password"}
        value={password}
        onChange={setPassword}
        disabled={busy}
      />
      {setup && (
        <PasswordInput
          label="Confirm password"
          value={confirmation}
          onChange={setConfirmation}
          disabled={busy}
        />
      )}
      <p role="alert" className="form-error">
        {error}
      </p>
      <button
        type="submit"
        className="primary-button continue-button"
        disabled={busy}
      >
        {recovery ? "Save new password" : setup ? "Continue" : "Unlock"}
      </button>
      <p role="status" className="setup-status">
        {message}
      </p>
    </form>
  );
}
function RecoveryForm({
  busy,
  error,
  submit,
  view,
}: {
  busy: boolean;
  error: string;
  submit: (p: string) => Promise<void>;
  view: () => void;
}) {
  const [paper, setPaper] = useState("");
  return (
    <form
      onSubmit={(e: FormEvent) => {
        e.preventDefault();
        if (busy) return;
        void submit(paper);
        setPaper("");
      }}
    >
      <div className="form-field">
        <label htmlFor="paper-code">Type your recovery code</label>
        <textarea
          id="paper-code"
          autoFocus
          value={paper}
          onChange={(e) => setPaper(e.target.value)}
          maxLength={256}
          rows={3}
          disabled={busy}
          autoComplete="off"
          spellCheck={false}
        />
      </div>
      <p role="alert" className="form-error">
        {error}
      </p>
      <button className="primary-button continue-button" disabled={busy}>
        Create vault
      </button>
      <button
        className="back-button view-code"
        type="button"
        disabled={busy}
        onClick={view}
      >
        View code again
      </button>
    </form>
  );
}

function RecoveryCodeForm({ busy, error, submit }: {
  busy: boolean;
  error: string;
  submit: (paper: string) => Promise<void>;
}) {
  const [paper, setPaper] = useState("");
  return (
    <form onSubmit={(event) => {
      event.preventDefault();
      if (busy) return;
      void submit(paper);
      setPaper("");
    }} noValidate>
      <PasswordInput label="Recovery code" value={paper} onChange={setPaper} disabled={busy} />
      <p className="form-error" role="alert">{error}</p>
      <button className="primary-button continue-button" type="submit" disabled={busy}>Continue</button>
      <p className="setup-status" role="status">{busy ? "Checking recovery code…" : ""}</p>
    </form>
  );
}
