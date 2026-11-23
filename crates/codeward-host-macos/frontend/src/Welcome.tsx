import { useEffect, useState } from "react";
import { invoke, type Status } from "./native";
import { Logo } from "./Logo";

type Entry = "checking" | "missing" | "existing" | "unavailable";

export function Welcome({
  create,
  open,
}: {
  create: () => void;
  open: () => void;
}) {
  const [entry, setEntry] = useState<Entry>("checking");
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    // Browser-only previews cannot contain a native working vault.
    if (!window.__TAURI__) {
      setEntry("missing");
      return;
    }
    let active = true;
    let epoch: number | undefined;
    let revocation: number | undefined;
    let timer: number | undefined;
    let preserveRecovery = false;
    async function release() {
      if (epoch === undefined || preserveRecovery) return;
      const checkedEpoch = epoch;
      epoch = undefined;
      await invoke("cancel_vault_setup", { epoch: checkedEpoch });
    }
    async function failed() {
      await release().catch(() => {});
      if (active) setEntry("unavailable");
    }
    async function check(status: Status) {
      if (!active) return;
      if (status.epoch !== epoch || status.revocation !== revocation) {
        await failed();
        return;
      }
      if (status.state === "opening") {
        timer = window.setTimeout(() => {
          void invoke<Status>("get_vault_setup_state").then(check).catch(failed);
        }, 150);
        return;
      }
      // An unresolved password replacement must survive a read-only cover check.
      preserveRecovery = status.state.startsWith("recovery_") || status.state === "recovering";
      const next: Entry = preserveRecovery
        ? "existing"
        : status.error
        ? "unavailable"
        : status.state === "missing"
          ? "missing"
          : status.state === "locked"
            ? "existing"
            : "unavailable";
      // Release the read-only opening before enabling setup or another opening.
      await release();
      if (active) setEntry(next);
    }
    setEntry("checking");
    void invoke<Status>("open_managed_vault")
      .then(async (status) => {
        epoch = status.epoch;
        revocation = status.revocation;
        preserveRecovery = status.state.startsWith("recovery_") || status.state === "recovering";
        if (active) await check(status);
        else await release();
      })
      .catch(failed);
    return () => {
      active = false;
      window.clearTimeout(timer);
      void release().catch(() => {});
    };
  }, [attempt]);

  return (
    <section className="welcome">
      <Logo className="welcome-logo" />
      <h1 className="wordmark entrance">
        <span className="code">Code</span>
        <span className="ward">ward</span>
      </h1>
      <p className="byline entrance">Your local password manager</p>
      <div className="action-area entrance" aria-busy={entry === "checking"}>
        <button
          className="primary-button"
          disabled={entry === "checking"}
          onClick={
            entry === "missing"
              ? create
              : entry === "existing"
                ? open
                : () => setAttempt((n) => n + 1)
          }
        >
          {entry === "checking"
            ? "Checking vault…"
            : entry === "missing"
              ? "Create your vault"
              : entry === "existing"
                ? "Unlock Vault"
                : "Try again"}
        </button>
        {entry === "unavailable" && (
          <p className="welcome-status" role="status">
            Couldn't check your vault. Try again or reopen Codeward.
          </p>
        )}
      </div>
    </section>
  );
}
