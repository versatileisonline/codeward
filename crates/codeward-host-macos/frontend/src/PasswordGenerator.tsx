import { useEffect, useRef, useState } from "react";
import {
  invoke,
  type Scope,
  type GeneratedDraft,
  type PasswordOptions,
} from "./native";
const defaults: PasswordOptions = {
  length: 24,
  uppercase: true,
  lowercase: true,
  digits: true,
  symbols: true,
};
function sameScope(a: Scope, b: Scope) {
  return (
    a.epoch === b.epoch &&
    a.revocation === b.revocation &&
    a.generation === b.generation &&
    a.revision === b.revision
  );
}
export function PasswordGenerator({
  scope,
  onDraft,
  onBusy,
  disabled,
}: {
  scope: Scope;
  onDraft: (draft: GeneratedDraft | null) => void;
  onBusy: (busy: boolean) => void;
  disabled: boolean;
}) {
  const [options, setOptions] = useState(defaults),
    [reference, setReference] = useState<GeneratedDraft | null>(null),
    [generating, setGenerating] = useState(false),
    [revealing, setRevealing] = useState(false),
    [preview, setPreview] = useState<string | null>(null),
    [error, setError] = useState("");
  const current = useRef<GeneratedDraft | null>(null),
    alive = useRef(true),
    pending = useRef(false),
    generation = useRef(0),
    revealSequence = useRef(0),
    timer = useRef<number | undefined>(undefined),
    callbacks = useRef({ onDraft, onBusy });
  callbacks.current = { onDraft, onBusy };
  function hide() {
    revealSequence.current++;
    window.clearTimeout(timer.current);
    setPreview(null);
    setRevealing(false);
  }
  function discard(ref: GeneratedDraft | null) {
    if (ref) void invoke("discard_password_draft", { ...ref }).catch(() => {});
  }
  useEffect(() => {
    alive.current = true;
    const blur = () => hide();
    const hidden = () => {
      if (document.hidden) hide();
    };
    window.addEventListener("blur", blur);
    document.addEventListener("visibilitychange", hidden);
    return () => {
      alive.current = false;
      generation.current++;
      revealSequence.current++;
      window.clearTimeout(timer.current);
      discard(current.current);
      current.current = null;
      callbacks.current.onBusy(false);
      window.removeEventListener("blur", blur);
      document.removeEventListener("visibilitychange", hidden);
    };
  }, []);
  useEffect(() => {
    if (disabled) hide();
  }, [disabled]);
  const valid =
    Number.isInteger(options.length) &&
    options.length >= 12 &&
    options.length <= 128 &&
    (options.uppercase ||
      options.lowercase ||
      options.digits ||
      options.symbols);
  async function generate() {
    if (pending.current || disabled || !valid) return;
    pending.current = true;
    setGenerating(true);
    callbacks.current.onBusy(true);
    setError("");
    hide();
    const token = ++generation.current;
    try {
      const next = await invoke<GeneratedDraft>("generate_password_draft", {
        epoch: scope.epoch,
        revocation: scope.revocation,
        generation: scope.generation,
        revision: scope.revision,
        options,
      });
      if (!alive.current || token !== generation.current) {
        discard(next);
        return;
      }
      if (!sameScope(next, scope) || typeof next.draft !== "string") {
        discard(next);
        setError("Could not generate a password. Try again.");
        return;
      }
      current.current = next;
      setReference(next);
      callbacks.current.onDraft(next);
    } catch {
      if (alive.current && token === generation.current)
        setError(
          "Could not generate a password. Your previous choice is unchanged. Try again.",
        );
    } finally {
      pending.current = false;
      if (alive.current) {
        setGenerating(false);
        callbacks.current.onBusy(false);
      }
    }
  }
  async function reveal() {
    if (preview !== null) {
      hide();
      return;
    }
    const ref = current.current;
    if (!ref || pending.current || disabled) return;
    const token = ++revealSequence.current,
      started = performance.now();
    setRevealing(true);
    setError("");
    try {
      const reply = await invoke<
        GeneratedDraft & { password: string; expires_in_ms: number }
      >("reveal_password_draft", { ...ref });
      const remaining =
        Math.min(30000, reply.expires_in_ms) - (performance.now() - started);
      if (
        !alive.current ||
        token !== revealSequence.current ||
        current.current?.draft !== ref.draft ||
        !sameScope(reply, ref) ||
        reply.draft !== ref.draft ||
        document.hidden ||
        remaining <= 0
      )
        return;
      setPreview(reply.password);
      timer.current = window.setTimeout(hide, remaining);
    } catch {
      if (alive.current && token === revealSequence.current)
        setError(
          "Generated password preview is unavailable. Generate a new password if this draft is no longer valid.",
        );
    } finally {
      if (alive.current && token === revealSequence.current)
        setRevealing(false);
    }
  }
  function manual() {
    if (pending.current || disabled) return;
    hide();
    discard(current.current);
    current.current = null;
    setReference(null);
    setError("");
    callbacks.current.onDraft(null);
  }
  return (
    <section className="password-generator" aria-label="Password generator">
      <div className="generator-heading">
        <strong>Password generator</strong>
        <span className="field-hint">
          Saved only when you save this credential.
        </span>
      </div>
      <div className="generator-options">
        <label htmlFor="generator-length">
          Password length
          <input
            id="generator-length"
            type="number"
            min={12}
            max={128}
            value={Number.isNaN(options.length) ? "" : options.length}
            disabled={disabled || generating}
            onChange={(e) =>
              setOptions((old) => ({ ...old, length: Number(e.target.value) }))
            }
          />
        </label>
        <div className="generator-classes">
          {(["uppercase", "lowercase", "digits", "symbols"] as const).map(
            (key) => (
              <label key={key} className="checkbox-field">
                <input
                  type="checkbox"
                  checked={options[key]}
                  disabled={disabled || generating}
                  onChange={(e) =>
                    setOptions((old) => ({ ...old, [key]: e.target.checked }))
                  }
                />
                {key[0].toUpperCase() + key.slice(1)}
              </label>
            ),
          )}
        </div>
      </div>
      {!valid && (
        <p className="field-hint">
          Choose 12–128 characters and at least one character class.
        </p>
      )}
      <p className="field-hint">
        Settings apply when you generate. Selected classes form the available
        alphabet.
      </p>
      <div className="generator-actions">
        <button
          className="secondary-button"
          type="button"
          disabled={disabled || generating || !valid}
          onClick={() => void generate()}
        >
          {generating
            ? "Generating…"
            : reference
              ? "Regenerate password"
              : "Generate password"}
        </button>
        {reference && (
          <button
            className="secondary-button"
            type="button"
            disabled={disabled || generating}
            onClick={manual}
          >
            Use typed password
          </button>
        )}
      </div>
      {reference && (
        <div className="generated-preview">
          <p className="field-hint">Generated password ready</p>
          <div className="password-field">
            <output aria-label="Generated password" className="generated-value">
              {preview ?? "••••••••••••••••"}
            </output>
            <button
              className="reveal-button"
              type="button"
              disabled={disabled || generating || revealing}
              aria-label={
                preview === null
                  ? "Show generated password"
                  : "Hide generated password"
              }
              aria-pressed={preview !== null}
              onClick={() => void reveal()}
            >
              {preview === null ? "Show" : "Hide"}
            </button>
          </div>
        </div>
      )}
      <p role="status" className="form-error">
        {error}
      </p>
    </section>
  );
}
