// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { Vault } from "./Vault";
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete window.__TAURI__;
});
const context = { epoch: 1, revocation: 0 };
const scope = { ...context, generation: "9007199254740993", revision: "3" };
const entry = {
  id: "1",
  title: "Sample mail",
  username: "sample",
  url: "https://example.test",
  notes: "Sample notes",
  created_at: "1",
  updated_at: "2",
  ...scope,
};
function mount(handler: (c: string, b: Record<string, unknown>) => unknown) {
  window.__TAURI__ = {
    core: { invoke: vi.fn(async (c, b = {}) => handler(c, b)) as never },
  };
  return render(
    <Vault
      status={{ ...context, state: "unlocked" }}
      lock={() => {}}
      guard={{ current: null }}
    />,
  );
}
test("lists and reads one selected credential without revealing passwords", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : entry,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  await screen.findByText("Sample notes");
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "get_credential_details",
    { ...scope, id: "1" },
  );
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith(
    "reveal_credential_password",
    expect.anything(),
  );
});
test("changing selection rejects delayed details", async () => {
  let done!: (v: unknown) => void;
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "get_credential_details"
        ? new Promise((r) => (done = r))
        : {},
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  done(entry);
  await waitFor(() =>
    expect(
      screen.getByRole("heading", { name: "New credential" }),
    ).toBeTruthy(),
  );
  expect(screen.queryByText("Sample notes")).toBeNull();
});
test("create waits for committed status and retains input after a definite failure", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [], next: null }
      : c === "create_credential"
        ? { ...context, operation: "1", state: "saving" }
        : c === "get_credential_operation"
          ? { ...context, operation: "1", state: "failed" }
          : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Sample" },
  });
  fireEvent.change(screen.getByLabelText("Password"), {
    target: { value: "synthetic" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
  await screen.findByText(
    "Save failed. Your changes are still here. Try again.",
  );
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe(
    "Sample",
  );
  expect(screen.queryByText("Saved")).toBeNull();
});
test("a reveal delivered after window blur never repopulates the password", async () => {
  let done!: (v: unknown) => void;
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "reveal_credential_password"
        ? new Promise((r) => (done = r))
        : entry,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  await screen.findByText("Sample notes");
  fireEvent.click(screen.getByRole("button", { name: "Show password" }));
  fireEvent(window, new Event("blur"));
  done({
    ...scope,
    id: "1",
    password: "SYNTHETIC REVEAL",
    expires_in_ms: 30000,
  });
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Show password" })).toBeTruthy(),
  );
  expect(screen.queryByText("SYNTHETIC REVEAL")).toBeNull();
});
test("a controlled validation rejection leaves the editor correctable", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [], next: null }
      : c === "create_credential"
        ? Promise.reject("invalid_input")
        : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
  await screen.findByText("Check the field lengths and try again.");
  expect((screen.getByLabelText("Title") as HTMLInputElement).disabled).toBe(
    false,
  );
});
test("unknown save permits native close after confirmation has finished", async () => {
  let done!: (v: unknown) => void;
  const ui = mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [], next: null }
      : c === "create_credential"
        ? new Promise((r) => (done = r))
        : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Sample" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
  ui.rerender(
    <Vault
      status={{ ...context, state: "unlocked", close_requested: true }}
      lock={() => {}}
      guard={{ current: null }}
    />,
  );
  done({ ...context, operation: "1", state: "unknown" });
  await screen.findByText(
    "Save not confirmed. Quit and reopen Codeward to check the vault before making more changes.",
  );
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "resolve_desktop_close",
      { ...context, allow: true },
    ),
  );
});
test("dirty navigation supports cancel and discard without writing", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : entry,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  await screen.findByText("Sample notes");
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Changed" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  await screen.findByRole("dialog");
  fireEvent.click(
    screen.getAllByRole("button", { name: "Cancel" }).at(-1)!,
  );
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe(
    "Changed",
  );
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe("");
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith(
    "update_credential",
    expect.anything(),
  );
});
test("save changes on navigation preserves an existing password without reveal", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "update_credential"
        ? { ...context, operation: "1", state: "saving" }
        : c === "get_credential_operation"
          ? { ...context, operation: "1", state: "saved" }
          : entry,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  await screen.findByText("Sample notes");
  fireEvent.click(screen.getByRole("button", { name: "Edit" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Changed" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  await screen.findByRole("heading", { name: "New credential" });
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "update_credential",
    {
      ...scope,
      id: "1",
      form: {
        title: "Changed",
        username: entry.username,
        url: entry.url,
        notes: entry.notes,
        password: { mode: "keep" },
      },
    },
  );
});
test("delete requires confirmation and reports success after commit", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "delete_credential"
        ? { ...context, operation: "1", state: "saving" }
        : c === "get_credential_operation"
          ? { ...context, operation: "1", state: "saved" }
          : entry,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  await screen.findByText("Sample notes");
  fireEvent.click(screen.getByRole("button", { name: "Delete credential" }));
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith(
    "delete_credential",
    expect.anything(),
  );
  fireEvent.click(
    screen.getAllByRole("button", { name: "Delete credential" }).at(-1)!,
  );
  await screen.findByText("Credential deleted");
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "delete_credential",
    { ...scope, id: "1" },
  );
});
test("native close can be canceled without losing an edit", async () => {
  const ui = mount((c) =>
    c === "list_credentials" ? { ...scope, summaries: [], next: null } : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Still here" },
  });
  ui.rerender(
    <Vault
      status={{ ...context, state: "unlocked", close_requested: true }}
      lock={() => {}}
      guard={{ current: null }}
    />,
  );
  await screen.findByRole("dialog");
  fireEvent.click(
    screen.getAllByRole("button", { name: "Cancel" }).at(-1)!,
  );
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "resolve_desktop_close",
      { ...context, allow: false },
    ),
  );
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe(
    "Still here",
  );
});
test("search filters actual title and username without artificial separator matches", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : {},
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "n" },
  });
  expect(screen.getByText("No matching credentials.")).toBeTruthy();
});
