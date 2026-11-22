import { useEffect, useRef, useState, type MutableRefObject } from "react";
import {
  invoke,
  type Details,
  type Form,
  type Operation,
  type Page,
  type Scope,
  type Status,
  type Summary,
} from "./native";
type Guard = MutableRefObject<((action: () => void) => void) | null>;
type Draft = {
  title: string;
  username: string;
  url: string;
  notes: string;
  password: string;
  replace: boolean;
};
const empty: Draft = {
  title: "",
  username: "",
  url: "",
  notes: "",
  password: "",
  replace: true,
};
const failure = (e: unknown) =>
  e === "stale" || e === "conflict"
    ? "The vault changed. Reload the list before editing again."
    : e === "invalid_input"
      ? "Check the field lengths and try again."
      : e === "locked"
        ? "Your vault is locked. Unlock it to continue."
        : "The request could not finish. Try again.";
export function Vault({
  status,
  lock,
  guard,
}: {
  status: Status;
  lock: () => void;
  guard: Guard;
}) {
  const [page, setPage] = useState<Page | null>(null),
    [rows, setRows] = useState<Summary[]>([]),
    [search, setSearch] = useState(""),
    [selected, setSelected] = useState<Details | null>(null),
    [draft, setDraft] = useState<Draft | null>(null),
    [dirty, setDirty] = useState(false),
    [busy, setBusy] = useState(false),
    [loading, setLoading] = useState(true),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [blocked, setBlocked] = useState(false),
    [revealed, setRevealed] = useState<string | null>(null),
    [dialog, setDialog] = useState<"dirty" | "delete" | null>(null);
  const dialogFocus = useRef<HTMLElement | null>(null);
  function openDialog(value: "dirty" | "delete") {
    dialogFocus.current = document.activeElement as HTMLElement;
    setDialog(value);
  }
  useEffect(() => {
    if (!dialog && dialogFocus.current?.isConnected) {
      dialogFocus.current.focus();
      dialogFocus.current = null;
    }
  }, [dialog]);
  const revealVersion = useRef(0),
    lastActivity = useRef(0);
  const version = useRef(0),
    mounted = useRef(true),
    revealTimer = useRef<number | undefined>(undefined),
    desired = useRef<(() => void) | null>(null),
    sending = useRef(false),
    dirtyRef = useRef(false),
    draftRef = useRef<Draft | null>(null),
    selectedRef = useRef<Details | null>(null),
    pageRef = useRef<Page | null>(null);
  const context = { epoch: status.epoch, revocation: status.revocation };
  function hidePassword() {
    revealVersion.current++;
    window.clearTimeout(revealTimer.current);
    setRevealed(null);
  }
  function markDirty(value: boolean) {
    dirtyRef.current = value;
    setDirty(value);
    void invoke("set_desktop_dirty", { ...context, dirty: value }).catch(
      () => {},
    );
  }
  function reset() {
    version.current++;
    setLoading(false);
    hidePassword();
    setSelected(null);
    selectedRef.current = null;
    setDraft(null);
    draftRef.current = null;
    markDirty(false);
    setError("");
    setNotice("");
  }
  async function load(append = false) {
    const token = version.current;
    setLoading(true);
    setError("");
    try {
      const next = await invoke<Page>("list_credentials", {
        ...context,
        cursor: append ? (pageRef.current?.next ?? null) : null,
      });
      if (!mounted.current || token !== version.current) return;
      if (next.epoch !== status.epoch || next.revocation !== status.revocation)
        return;
      pageRef.current = next;
      setPage(next);
      setRows((old) => (append ? [...old, ...next.summaries] : next.summaries));
    } catch (e) {
      if (mounted.current && token === version.current) setError(failure(e));
    } finally {
      if (mounted.current && token === version.current) setLoading(false);
    }
  }
  useEffect(() => {
    mounted.current = true;
    void load();
    const blur = () => hidePassword();
    window.addEventListener("blur", blur);
    return () => {
      mounted.current = false;
      version.current++;
      window.clearTimeout(revealTimer.current);
      guard.current = null;
      window.removeEventListener("blur", blur);
    };
  }, []);
  function request(action: () => void) {
    if (sending.current) return;
    if (blocked) {
      action();
      return;
    }
    if (dirtyRef.current) {
      hidePassword();
      desired.current = action;
      openDialog("dirty");
    } else action();
  }
  useEffect(() => {
    guard.current = request;
    return () => {
      guard.current = null;
    };
  }, [blocked]);
  useEffect(() => {
    if (status.close_requested) {
      request(() => {
        void invoke("resolve_desktop_close", { ...context, allow: true });
      });
    }
  }, [status.close_requested, busy, blocked]);
  async function select(row: Summary) {
    reset();
    const token = version.current;
    setLoading(true);
    try {
      const p = pageRef.current;
      if (!p) return;
      const detail = await invoke<Details>("get_credential_details", {
        epoch: p.epoch,
        revocation: p.revocation,
        generation: p.generation,
        revision: p.revision,
        id: row.id,
      });
      if (
        mounted.current &&
        token === version.current &&
        detail.epoch === status.epoch &&
        detail.revocation === status.revocation
      ) {
        setSelected(detail);
        selectedRef.current = detail;
      }
    } catch (e) {
      if (mounted.current && token === version.current) setError(failure(e));
    } finally {
      if (mounted.current && token === version.current) setLoading(false);
    }
  }
  function startNew() {
    reset();
    setDraft({ ...empty });
    draftRef.current = { ...empty };
  }
  function edit() {
    if (!selected) return;
    hidePassword();
    const value = {
      title: selected.title,
      username: selected.username,
      url: selected.url,
      notes: selected.notes,
      password: "",
      replace: false,
    };
    setDraft(value);
    draftRef.current = value;
    setNotice("");
  }
  function change(key: keyof Draft, value: string | boolean) {
    const next = { ...draftRef.current!, [key]: value };
    draftRef.current = next;
    setDraft(next);
    markDirty(true);
    setError("");
  }
  async function reveal() {
    if (!selected || revealed !== null) {
      hidePassword();
      return;
    }
    const token = version.current,
      request = ++revealVersion.current,
      started = performance.now();
    try {
      const reply = await invoke<
        Scope & { id: string; password: string; expires_in_ms: number }
      >("reveal_credential_password", selectionScope(selected));
      const remaining = reply.expires_in_ms - (performance.now() - started);
      if (
        !mounted.current ||
        token !== version.current ||
        request !== revealVersion.current ||
        document.visibilityState === "hidden" ||
        remaining <= 0
      )
        return;
      if (
        reply.epoch !== status.epoch ||
        reply.revocation !== status.revocation ||
        reply.id !== selected.id ||
        reply.revision !== selected.revision ||
        reply.generation !== selected.generation
      )
        return;
      setRevealed(reply.password);
      revealTimer.current = window.setTimeout(
        hidePassword,
        Math.min(remaining, 30000),
      );
    } catch (e) {
      if (mounted.current && token === version.current) setError(failure(e));
    }
  }
  async function mutate(kind: "save" | "delete"): Promise<boolean> {
    if (sending.current || blocked || !pageRef.current) return false;
    const p = pageRef.current,
      detail = selectedRef.current,
      d = draftRef.current;
    const token = version.current;
    sending.current = true;
    setBusy(true);
    setError("");
    setNotice("Saving…");
    hidePassword();
    try {
      const scope = {
        epoch: p.epoch,
        revocation: p.revocation,
        generation: p.generation,
        revision: p.revision,
      };
      let operation: Operation;
      if (kind === "delete") {
        if (!detail) return false;
        operation = await invoke("delete_credential", {
          ...scope,
          id: detail.id,
        });
      } else {
        if (!d) return false;
        const form: Form = {
          title: d.title,
          username: d.username,
          url: d.url,
          notes: d.notes,
          password: d.replace
            ? { mode: "provided", value: d.password }
            : { mode: "keep" },
        };
        operation = await invoke(
          detail ? "update_credential" : "create_credential",
          { ...scope, ...(detail ? { id: detail.id } : {}), form },
        );
      }
      if (!mounted.current || token !== version.current) return false;
      for (;;) {
        if (
          operation.epoch !== status.epoch ||
          operation.revocation !== status.revocation
        )
          return false;
        if (operation.state !== "saving") break;
        await new Promise((r) => window.setTimeout(r, 150));
        if (!mounted.current || token !== version.current) return false;
        operation = await invoke<Operation>("get_credential_operation", {
          epoch: p.epoch,
          operation: operation.operation,
        });
      }
      if (!mounted.current || token !== version.current) return false;
      if (operation.state === "saved") {
        reset();
        setNotice(kind === "delete" ? "Credential deleted" : "Saved");
        await load();
        return true;
      }
      setNotice("");
      if (operation.state === "unknown") {
        setBlocked(true);
        setError(
          "Save not confirmed. Quit and reopen Codeward to check the vault before making more changes.",
        );
      } else if (operation.state === "conflict") {
        setBlocked(true);
        setError(
          "The vault changed. Quit and reopen Codeward before making more changes.",
        );
      } else if (operation.state === "locked") {
        setError("Your vault is locked. Unlock it to continue.");
        lock();
      } else setError("Save failed. Your changes are still here. Try again.");
      return false;
    } catch (e) {
      if (mounted.current && token === version.current) {
        setNotice("");
        if (
          ["invalid_input", "stale", "busy", "unauthorized", "locked"].includes(
            String(e),
          )
        ) {
          setError(failure(e));
        } else {
          setBlocked(true);
          setError(
            "Save status unavailable. Quit and reopen Codeward to check the vault before making more changes.",
          );
        }
      }
      return false;
    } finally {
      if (mounted.current) {
        setBusy(false);
        sending.current = false;
      }
    }
  }
  async function resolveDirty(choice: "save" | "discard" | "cancel") {
    if (choice === "cancel") {
      desired.current = null;
      setDialog(null);
      if (status.close_requested)
        void invoke("resolve_desktop_close", {
          ...context,
          allow: false,
        }).catch(() => {});
      return;
    }
    const action = desired.current;
    if (choice === "save" && !(await mutate("save"))) {
      setDialog(null);
      return;
    }
    if (choice === "discard") {
      setDraft(null);
      draftRef.current = null;
      markDirty(false);
    }
    desired.current = null;
    setDialog(null);
    action?.();
  }
  function activity(kind: "keyboard" | "pointer") {
    const p = pageRef.current,
      now = performance.now();
    if (!p || now - lastActivity.current < 1000) return;
    lastActivity.current = now;
    void invoke("record_desktop_activity", {
      ...context,
      generation: p.generation,
      kind,
    }).catch(() => {});
  }
  const filtered = rows.filter((r) =>
    `${r.title} ${r.username}`
      .toLocaleLowerCase()
      .includes(search.toLocaleLowerCase()),
  );
  return (
    <section
      className="vault-workspace"
      aria-labelledby="vault-heading"
      onKeyDownCapture={(e) => {
        if (e.isTrusted) activity("keyboard");
      }}
      onPointerDownCapture={(e) => {
        if (e.isTrusted) activity("pointer");
      }}
    >
      <header className="vault-toolbar">
        <div>
          <h2 id="vault-heading" tabIndex={-1}>
            Your vault
          </h2>
        </div>
        <button className="secondary-button" onClick={lock}>
          Lock vault <span aria-hidden="true">↗</span>
        </button>
      </header>
      <div className="vault-columns">
        <aside className="credential-list" aria-label="Credentials">
          <div className="list-tools">
            <button
              className="primary-button"
              disabled={busy || blocked || !page}
              onClick={() => request(startNew)}
            >
              <span aria-hidden="true">＋ </span>Add credential
            </button>
            <label className="visually-hidden" htmlFor="credential-search">
              Search credentials
            </label>
            <input
              id="credential-search"
              type="search"
              placeholder="Search credentials"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
          <p className="list-caption">
            {rows.length} {rows.length === 1 ? "credential" : "credentials"}
            {page?.next ? " loaded" : ""}
          </p>
          <div className="credential-rows">
            {filtered.map((row) => (
              <button
                key={row.id}
                className={`credential-row ${selected?.id === row.id ? "selected" : ""}`}
                aria-pressed={selected?.id === row.id}
                disabled={busy || blocked}
                onClick={() => request(() => void select(row))}
              >
                <span className="credential-avatar" aria-hidden="true">
                  {(row.title || "C").slice(0, 1).toUpperCase()}
                </span>
                <span className="credential-summary">
                  <strong>{row.title || "Untitled credential"}</strong>
                  <span>{row.username || "No username"}</span>
                </span>
                <span aria-hidden="true" className="row-arrow">
                  ›
                </span>
              </button>
            ))}
          </div>
          {!loading && !filtered.length && rows.length > 0 && (
            <p className="list-caption">No matching credentials.</p>
          )}
          {page?.next && (
            <button
              className="secondary-button load-more"
              disabled={loading || busy || blocked}
              onClick={() => void load(true)}
            >
              Load more
            </button>
          )}
        </aside>
        <div className="credential-panel" aria-busy={busy || loading}>
          <div className="panel-feedback">
            <p role="status" className="setup-status">
              {notice || (loading ? "Loading credentials…" : "")}
            </p>
            <p role="alert" className="form-error">
              {error}
            </p>
          </div>
          {draft ? (
            <form
              className="credential-editor"
              onSubmit={(e) => {
                e.preventDefault();
                void mutate("save");
              }}
            >
              <div className="detail-heading">
                <div>
                  <p className="step-label">
                    {selected ? "EDIT CREDENTIAL" : "NEW CREDENTIAL"}
                  </p>
                  <h3>{selected ? "Edit credential" : "New credential"}</h3>
                </div>
                {dirty && (
                  <span className="unsaved-label">Unsaved changes</span>
                )}
              </div>
              <fieldset disabled={busy || blocked}>
                <TextField
                  label="Title"
                  value={draft.title}
                  onChange={(v) => change("title", v)}
                />
                <TextField
                  label="Username"
                  value={draft.username}
                  onChange={(v) => change("username", v)}
                />
                <TextField
                  label="URL"
                  value={draft.url}
                  onChange={(v) => change("url", v)}
                />
                {selected && (
                  <label className="checkbox-field">
                    <input
                      type="checkbox"
                      checked={draft.replace}
                      onChange={(e) => change("replace", e.target.checked)}
                    />
                    Replace password
                  </label>
                )}
                {draft.replace ? (
                  <TextField
                    label="Password"
                    type="password"
                    value={draft.password}
                    onChange={(v) => change("password", v)}
                  />
                ) : (
                  <p className="field-hint">
                    Your current password will be kept.
                  </p>
                )}
                <TextField
                  label="Notes"
                  multiline
                  value={draft.notes}
                  onChange={(v) => change("notes", v)}
                />
                <div className="editor-actions">
                  <button className="primary-button" type="submit">
                    Save credential
                  </button>
                  <button
                    className="secondary-button"
                    type="button"
                    onClick={() =>
                      request(() => {
                        setDraft(null);
                        draftRef.current = null;
                        markDirty(false);
                      })
                    }
                  >
                    Cancel
                  </button>
                </div>
              </fieldset>
            </form>
          ) : selected ? (
            <article className="credential-details">
              <div className="detail-heading">
                <div>
                  <p className="step-label">CREDENTIAL</p>
                  <h3>{selected.title || "Untitled credential"}</h3>
                </div>
                <button
                  className="secondary-button"
                  disabled={busy || blocked}
                  onClick={edit}
                >
                  Edit
                </button>
              </div>
              <dl>
                <Detail label="Username" value={selected.username} />
                <div className="detail-field">
                  <dt>Password</dt>
                  <dd className="password-detail">
                    <span className="password-text">
                      {revealed ?? "••••••••••••"}
                    </span>
                    <button
                      className="reveal-button"
                      disabled={busy || blocked}
                      aria-pressed={revealed !== null}
                      onClick={reveal}
                    >
                      {revealed !== null ? "Hide password" : "Show password"}
                    </button>
                  </dd>
                </div>
                <Detail label="URL" value={selected.url} />
                <Detail label="Notes" value={selected.notes} />
              </dl>
              <button
                className="danger-button"
                disabled={busy || blocked}
                onClick={() => {
                  hidePassword();
                  openDialog("delete");
                }}
              >
                Delete credential
              </button>
            </article>
          ) : (
            !loading && (
              <div className="vault-empty">
                <div className="empty-symbol" aria-hidden="true">
                  ◇
                </div>
                <h3>
                  {rows.length
                    ? "Everything in its place"
                    : "Your vault is ready"}
                </h3>
                <p>
                  {rows.length
                    ? "Choose a credential to view its details."
                    : "Add your first credential. It stays on this Mac, protected in your vault."}
                </p>
                {!rows.length && (
                  <p className="preview-note">
                    Use sample credentials in this development build.
                  </p>
                )}
                {error && (
                  <button
                    className="secondary-button"
                    disabled={blocked}
                    onClick={() => void load()}
                  >
                    Reload list
                  </button>
                )}
              </div>
            )
          )}
        </div>
      </div>
      {dialog && (
        <div className="dialog-backdrop">
          <section
            className="confirm-dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="dialog-title"
            onKeyDown={(e) => {
              if (e.key === "Escape" && !busy) {
                if (dialog === "dirty") void resolveDirty("cancel");
                else setDialog(null);
              }
              if (e.key === "Tab") {
                const els = Array.from(
                  e.currentTarget.querySelectorAll<HTMLButtonElement>(
                    "button:not(:disabled)",
                  ),
                );
                if (e.shiftKey && document.activeElement === els[0]) {
                  e.preventDefault();
                  els.at(-1)?.focus();
                } else if (
                  !e.shiftKey &&
                  document.activeElement === els.at(-1)
                ) {
                  e.preventDefault();
                  els[0]?.focus();
                }
              }
            }}
          >
            <h3 id="dialog-title">
              {dialog === "dirty"
                ? "Save your changes?"
                : "Delete this credential?"}
            </h3>
            <p>
              {dialog === "dirty"
                ? "Your changes have not been saved yet."
                : "This removes the credential from your vault. This cannot be undone."}
            </p>
            <div className="dialog-actions">
              {dialog === "dirty" ? (
                <>
                  <button
                    className="primary-button"
                    disabled={busy || blocked}
                    onClick={() => void resolveDirty("save")}
                  >
                    Save changes
                  </button>
                  <button
                    className="secondary-button"
                    disabled={busy}
                    onClick={() => void resolveDirty("discard")}
                  >
                    Discard changes
                  </button>
                  <button
                    className="secondary-button"
                    autoFocus
                    disabled={busy}
                    onClick={() => void resolveDirty("cancel")}
                  >
                    Cancel
                  </button>
                </>
              ) : (
                <>
                  <button
                    className="danger-button"
                    disabled={busy || blocked}
                    onClick={() => {
                      setDialog(null);
                      void mutate("delete");
                    }}
                  >
                    Delete credential
                  </button>
                  <button
                    className="secondary-button"
                    autoFocus
                    onClick={() => setDialog(null)}
                  >
                    Cancel
                  </button>
                </>
              )}
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
function selectionScope(detail: Details) {
  return {
    epoch: detail.epoch,
    revocation: detail.revocation,
    generation: detail.generation,
    revision: detail.revision,
    id: detail.id,
  };
}
function TextField({
  label,
  value,
  onChange,
  type = "text",
  multiline = false,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  type?: string;
  multiline?: boolean;
}) {
  const id = `credential-${label.toLowerCase()}`;
  return (
    <div className="form-field">
      <label htmlFor={id}>{label}</label>
      {multiline ? (
        <textarea
          id={id}
          rows={5}
          value={value}
          spellCheck={false}
          onChange={(e) => onChange(e.target.value)}
        />
      ) : (
        <input
          id={id}
          value={value}
          type={type}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => onChange(e.target.value)}
        />
      )}
    </div>
  );
}
function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div className="detail-field">
      <dt>{label}</dt>
      <dd>{value || "—"}</dd>
    </div>
  );
}
