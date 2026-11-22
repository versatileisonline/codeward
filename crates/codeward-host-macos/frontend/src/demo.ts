/** Development-only in-memory transport for reviewing the real React screens. */
import type { Form, Status } from "./native";
import "./demo.css";
type RecordEntry = {
  id: string;
  title: string;
  username: string;
  url: string;
  notes: string;
  password: string;
  created_at: string;
  updated_at: string;
};
type Args = Record<string, unknown>;
type DemoInvoke = <T = Status>(command: string, body?: Args) => Promise<T>;
const sampleRecords = (): RecordEntry[] => [
  {
    id: "sample-mail",
    title: "Sample mail",
    username: "sample@example.test",
    url: "https://mail.example.test",
    notes:
      "A sample credential for reviewing Codeward. No real account is connected.",
    password: "DEMO-MAIL-ONLY",
    created_at: "1",
    updated_at: "1",
  },
  {
    id: "sample-design",
    title: "Design workspace",
    username: "sample.designer",
    url: "https://design.example.test",
    notes: "Sample creative workspace.",
    password: "DEMO-DESIGN-ONLY",
    created_at: "1",
    updated_at: "1",
  },
  {
    id: "sample-code",
    title: "Code repository",
    username: "sample.developer",
    url: "https://code.example.test",
    notes: "Sample code projects.",
    password: "DEMO-CODE-ONLY",
    created_at: "1",
    updated_at: "1",
  },
];
export function createDemoAdapter(): DemoInvoke {
  let records = sampleRecords(),
    epoch = 0,
    revocation = 0,
    generation = 1,
    revision = 1,
    state = "locked",
    serial = 0,
    dirty = false,
    close_requested = false;
  const operations = new Map<string, { state: string; apply: () => void }>();
  const context = () => ({ epoch, revocation });
  const scope = () => ({
    ...context(),
    generation: String(generation),
    revision: String(revision),
  });
  const status = () => ({ ...context(), state, close_requested });
  function authorize(b: Args, selected = false) {
    if (state !== "unlocked") throw "locked";
    if (b.epoch !== epoch || b.revocation !== revocation) throw "stale";
    if (
      selected &&
      (b.generation !== String(generation) || b.revision !== String(revision))
    )
      throw "stale";
  }
  function selected(b: Args) {
    authorize(b, true);
    const record = records.find((x) => x.id === b.id);
    if (!record) throw "invalid_input";
    return record;
  }
  function queue(apply: () => void) {
    const operation = String(++serial);
    operations.set(operation, { state: "saving", apply });
    return { ...context(), operation, state: "saving" };
  }
  const handler = async (command: string, b: Args = {}): Promise<unknown> => {
    switch (command) {
      case "open_managed_vault":
        epoch++;
        state = "locked";
        return status();
      case "unlock_managed_vault":
        if (b.epoch !== epoch || b.revocation !== revocation) throw "stale";
        if (typeof b.password !== "string" || !b.password)
          throw "invalid_input";
        state = "unlocked";
        generation++;
        return status();
      case "get_vault_setup_state":
        return status();
      case "lock_managed_vault":
      case "cancel_vault_setup":
        state = "locked";
        revocation++;
        generation++;
        dirty = false;
        close_requested = false;
        return status();
      case "list_credentials":
        authorize(b);
        return {
          ...scope(),
          summaries: records.map(({ id, title, username }) => ({
            id,
            title,
            username,
          })),
          next: null,
        };
      case "get_credential_details": {
        const { password: _, ...record } = selected(b);
        return { ...scope(), ...record };
      }
      case "reveal_credential_password":
        return {
          ...scope(),
          id: selected(b).id,
          password: selected(b).password,
          expires_in_ms: 30000,
        };
      case "create_credential":
      case "update_credential": {
        authorize(b, true);
        const form = b.form as Form | undefined;
        if (!form || !form.title.trim() || form.title.length > 1024)
          throw "invalid_input";
        const existing =
          command === "update_credential" ? selected(b) : undefined;
        if (!existing && form.password.mode === "keep") throw "invalid_input";
        const password =
          form.password.mode === "keep"
            ? existing!.password
            : (form.password.value ?? "");
        const value: RecordEntry = {
          id: existing?.id ?? `demo-created-${++serial}`,
          title: form.title,
          username: form.username,
          url: form.url,
          notes: form.notes,
          password,
          created_at: existing?.created_at ?? String(Date.now()),
          updated_at: String(Date.now()),
        };
        return queue(() => {
          records = existing
            ? records.map((r) => (r.id === existing.id ? value : r))
            : [...records, value];
        });
      }
      case "delete_credential": {
        const record = selected(b);
        return queue(() => {
          records = records.filter((r) => r.id !== record.id);
        });
      }
      case "get_credential_operation": {
        if (b.epoch !== epoch) throw "stale";
        const op = operations.get(String(b.operation));
        if (!op) throw "invalid_input";
        if (op.state === "saving") {
          op.apply();
          op.state = "saved";
          revision++;
          dirty = false;
        }
        return {
          ...context(),
          operation: String(b.operation),
          state: op.state,
        };
      }
      case "set_desktop_dirty":
        authorize(b);
        dirty = b.dirty === true;
        return status();
      case "resolve_desktop_close":
        close_requested = false;
        return status();
      case "record_desktop_activity":
        authorize(b);
        return status();
      case "begin_vault_setup":
        epoch++;
        state = "recovery";
        return status();
      case "get_setup_recovery_code":
        if (state !== "recovery" || b.epoch !== epoch) throw "stale";
        return { epoch, code: "SAMPLE DEMO CODE — NOT A REAL RECOVERY SECRET" };
      case "confirm_vault_setup":
        if (state !== "recovery" || b.epoch !== epoch) throw "stale";
        state = "created";
        return status();
      default:
        throw "invalid_input";
    }
  };
  return handler as DemoInvoke;
}
export function installDemo(
  target: Pick<Window, "__TAURI__"> = window,
): boolean {
  if (target.__TAURI__) return false;
  target.__TAURI__ = { core: { invoke: createDemoAdapter() } };
  return true;
}
export function showDemoBadge() {
  const banner = document.createElement("aside");
  banner.className = "demo-banner";
  banner.setAttribute("aria-label", "Browser demo");
  banner.textContent =
    "Browser demo · Sample data only · Changes reset on reload";
  document.body.prepend(banner);
}
