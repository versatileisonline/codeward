// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import {
  act,
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
  vi.useRealTimers();
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
  fireEvent.click(screen.getAllByRole("button", { name: "Cancel" }).at(-1)!);
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
  fireEvent.click(screen.getAllByRole("button", { name: "Cancel" }).at(-1)!);
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
test("native search finds unloaded credentials and clearing restores listing", async () => {
  mount((c, b) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: "ordinary" }
      : c === "search_credentials"
        ? {
            ...scope,
            summaries:
              b.query === "remote"
                ? [{ ...entry, id: "remote", title: "Remote result" }]
                : [],
            next: null,
          }
        : {},
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "remote" },
  });
  await screen.findByRole("button", { name: /Remote result/ });
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "search_credentials",
    { ...context, query: "remote", cursor: null },
  );
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "absent" },
  });
  await screen.findByText("No matching credentials.");
  expect(screen.queryByText("Your vault is ready")).toBeNull();
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "" },
  });
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
});
test("search rejects delayed query and page replies and preserves dirty draft", async () => {
  let oldQuery!: (v: unknown) => void, oldPage!: (v: unknown) => void;
  mount((c, b) => {
    if (c === "list_credentials")
      return { ...scope, summaries: [entry], next: null };
    if (c === "search_credentials") {
      if (b.query === "first") return new Promise((r) => (oldQuery = r));
      if (b.cursor) return new Promise((r) => (oldPage = r));
      return {
        ...scope,
        summaries: [{ ...entry, id: "second", title: "Second result" }],
        next: "second-page",
      };
    }
    return {};
  });
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Keep draft" },
  });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "first" },
  });
  await waitFor(() => expect(oldQuery).toBeTypeOf("function"));
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "second" },
  });
  await screen.findByRole("button", { name: /Second result/ });
  await act(async () =>
    oldQuery({
      ...scope,
      summaries: [{ ...entry, title: "Obsolete result" }],
      next: null,
    }),
  );
  expect(screen.queryByRole("button", { name: /Obsolete result/ })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Load more" }));
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "" },
  });
  await act(async () =>
    oldPage({
      ...scope,
      summaries: [{ ...entry, title: "Obsolete page" }],
      next: null,
    }),
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  expect(screen.queryByRole("button", { name: /Obsolete page/ })).toBeNull();
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe(
    "Keep draft",
  );
  fireEvent.click(screen.getByRole("button", { name: /Sample mail.*sample/ }));
  await screen.findByRole("dialog");
});
test("saved native identity stays selected outside unchanged query", async () => {
  const saved = {
    ...entry,
    id: "native-assigned",
    title: "Duplicate title",
    notes: "Saved identity",
    revision: "4",
  };
  let committed = false;
  mount((c, b) => {
    if (c === "list_credentials")
      return { ...scope, summaries: [entry], next: null };
    if (c === "search_credentials")
      return {
        ...scope,
        revision: committed ? "4" : "3",
        summaries: [],
        next: null,
      };
    if (c === "create_credential") {
      committed = true;
      return {
        ...context,
        operation: "save",
        state: "saved",
        credential_id: saved.id,
      };
    }
    if (c === "get_credential_details")
      return b.id === saved.id ? saved : entry;
    return {};
  });
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "unrelated" },
  });
  await screen.findByText("No matching credentials.");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: saved.title },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
  await screen.findByText("Saved identity");
  expect(
    (screen.getByLabelText("Search credentials") as HTMLInputElement).value,
  ).toBe("unrelated");
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "get_credential_details",
    { ...scope, revision: "4", id: saved.id },
  );
  expect(screen.queryByLabelText("Title")).toBeNull();
});
test("shortcuts focus search and new title and guard dirty draft", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : {},
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.keyDown(window, { key: "f", metaKey: true });
  expect(document.activeElement).toBe(
    screen.getByLabelText("Search credentials"),
  );
  fireEvent.keyDown(window, { key: "n", metaKey: true });
  expect(document.activeElement).toBe(screen.getByLabelText("Title"));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Keep me" },
  });
  fireEvent.keyDown(window, { key: "n", metaKey: true });
  await screen.findByRole("dialog");
  fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe(
    "Keep me",
  );
  fireEvent.keyDown(screen.getByLabelText("Search credentials"), {
    key: "s",
    metaKey: true,
  });
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith(
    "create_credential",
    expect.anything(),
  );
});

test("typed credential password can be shown, hidden and remasked on focus loss", async () => {
  mount((c) =>
    c === "list_credentials" ? { ...scope, summaries: [], next: null } : {},
  );
  await waitFor(() =>
    expect(
      screen
        .getByRole("button", { name: "Add credential" })
        .hasAttribute("disabled"),
    ).toBe(false),
  );
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  const input = screen.getByLabelText("Password", {
    exact: true,
  }) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "synthetic typed password" } });
  expect(input.type).toBe("password");
  fireEvent.click(screen.getByRole("button", { name: "Show password" }));
  expect(input.type).toBe("text");
  expect(input.value).toBe("synthetic typed password");
  fireEvent.click(screen.getByRole("button", { name: "Hide password" }));
  expect(input.type).toBe("password");
  fireEvent.click(screen.getByRole("button", { name: "Show password" }));
  fireEvent.blur(window);
  expect(input.type).toBe("password");
});

test.each([false, true])(
  "generated password save uses opaque native reference and retains it on failure (edit=%s)",
  async (edit) => {
    let attempts = 0;
    mount((c, b) => {
      if (c === "list_credentials")
        return { ...scope, summaries: edit ? [entry] : [], next: null };
      if (c === "get_credential_details") return entry;
      if (c === "generate_password_draft") {
        expect(b).toEqual({
          ...scope,
          options: {
            length: 24,
            uppercase: true,
            lowercase: true,
            digits: true,
            symbols: true,
          },
        });
        return { ...scope, draft: "opaque-generated" };
      }
      if (c === "create_credential" || c === "update_credential") {
        expect((b.form as any).password).toEqual({
          mode: "generated",
          draft: "opaque-generated",
        });
        return { ...context, operation: String(++attempts), state: "saving" };
      }
      if (c === "get_credential_operation")
        return {
          ...context,
          operation: String(attempts),
          state: attempts === 1 ? "failed" : "saved",
        };
      return {};
    });
    await waitFor(() =>
      expect(
        (
          screen.getByRole("button", {
            name: "Add credential",
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(false),
    );
    if (edit) {
      fireEvent.click(
        screen.getByRole("button", { name: /Sample mail.*sample/ }),
      );
      await screen.findByText("Sample notes");
      fireEvent.click(screen.getByRole("button", { name: "Edit" }));
      fireEvent.click(screen.getByLabelText("Replace password"));
    } else {
      fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
      fireEvent.change(screen.getByLabelText("Title"), {
        target: { value: "Generated sample" },
      });
    }
    fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
    await screen.findByText("Generated password ready");
    expect(screen.queryByLabelText("Password", { exact: true })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
    await screen.findByText(
      "Save failed. Your changes are still here. Try again.",
    );
    expect(screen.getByText("Generated password ready")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Save credential" }));
    await screen.findByText("Saved", { exact: true });
    expect(attempts).toBe(2);
  },
);

test("search debounces rapid changes for 200ms", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : { ...scope, summaries: [], next: null },
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  vi.useFakeTimers();
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "r" },
  });
  await act(() => vi.advanceTimersByTimeAsync(150));
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "remote" },
  });
  await act(() => vi.advanceTimersByTimeAsync(199));
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith(
    "search_credentials",
    expect.anything(),
  );
  await act(() => vi.advanceTimersByTimeAsync(1));
  expect(screen.getByText("No matching credentials.")).toBeTruthy();
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "search_credentials",
    { ...context, query: "remote", cursor: null },
  );
});
test("search errors have a retry and do not masquerade as empty results", async () => {
  let failed = true;
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "search_credentials"
        ? failed
          ? Promise.reject("unavailable")
          : { ...scope, summaries: [], next: null }
        : {},
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "absent" },
  });
  await screen.findByText("The request could not finish. Try again.");
  expect(screen.queryByText("No matching credentials.")).toBeNull();
  expect(screen.queryByText("Your vault is ready")).toBeNull();
  failed = false;
  fireEvent.click(screen.getByRole("button", { name: "Reload list" }));
  await screen.findByText("No matching credentials.");
});
test("deleting a selected credential outside current search keeps query and clears detail", async () => {
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "search_credentials"
        ? { ...scope, summaries: [], next: null }
        : c === "get_credential_details"
          ? entry
          : c === "delete_credential"
            ? {
                ...context,
                operation: "delete",
                state: "saved",
                credential_id: null,
              }
            : {},
  );
  fireEvent.click(
    await screen.findByRole("button", { name: /Sample mail.*sample/ }),
  );
  await screen.findByText("Sample notes");
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "nonmatching" },
  });
  await screen.findByText("No matching credentials.");
  expect(screen.getByText("Sample notes")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Delete credential" }));
  fireEvent.click(
    screen.getAllByRole("button", { name: "Delete credential" }).at(-1)!,
  );
  await screen.findByText("Credential deleted");
  expect(screen.queryByText("Sample notes")).toBeNull();
  expect(
    (screen.getByLabelText("Search credentials") as HTMLInputElement).value,
  ).toBe("nonmatching");
});
test("new editor title receives focus after discarding an already open new draft", async () => {
  mount((c) =>
    c === "list_credentials" ? { ...scope, summaries: [], next: null } : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Discard me" },
  });
  screen.getByLabelText("Username").focus();
  fireEvent.keyDown(window, { key: "n", metaKey: true });
  fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
  expect(document.activeElement).toBe(screen.getByLabelText("Title"));
});
test("Escape hides a typed password without changing or discarding the draft", async () => {
  mount((c) =>
    c === "list_credentials" ? { ...scope, summaries: [], next: null } : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  const input = screen.getByLabelText("Password", {
    exact: true,
  }) as HTMLInputElement;
  fireEvent.change(input, { target: { value: "synthetic secret" } });
  fireEvent.click(screen.getByRole("button", { name: "Show password" }));
  fireEvent.keyDown(input, { key: "Escape" });
  expect(input.type).toBe("password");
  expect(input.value).toBe("synthetic secret");
  expect(screen.getByText("Unsaved changes")).toBeTruthy();
});
test("revocation clears drafts and rejects a late search response", async () => {
  let done!: (v: unknown) => void;
  const ui = mount((c, b) =>
    c === "list_credentials"
      ? { ...scope, ...b, summaries: [], next: null }
      : c === "search_credentials"
        ? new Promise((r) => (done = r))
        : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Volatile draft" },
  });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "old" },
  });
  await waitFor(() => expect(done).toBeTypeOf("function"));
  ui.rerender(
    <Vault
      status={{ ...context, revocation: 1, state: "unlocked" }}
      lock={() => {}}
      guard={{ current: null }}
    />,
  );
  await act(async () => done({ ...scope, summaries: [entry], next: null }));
  expect(screen.queryByLabelText("Title")).toBeNull();
  expect(
    screen.queryByRole("button", { name: /Sample mail.*sample/ }),
  ).toBeNull();
});

test("saving preserves the latest query and uses committed details without duplicate writes", async () => {
  let finish!: (v: unknown) => void;
  mount((c, b) =>
    c === "list_credentials"
      ? { ...scope, summaries: [entry], next: null }
      : c === "search_credentials"
        ? { ...scope, revision: "4", summaries: [], next: null }
        : c === "create_credential"
          ? new Promise((r) => (finish = r))
          : c === "get_credential_details"
            ? { ...entry, revision: "4", id: b.id, notes: "Committed details" }
            : {},
  );
  await screen.findByRole("button", { name: /Sample mail.*sample/ });
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "New entry" },
  });
  fireEvent.keyDown(screen.getByLabelText("Title"), {
    key: "s",
    metaKey: true,
  });
  expect(
    screen.getByRole("button", { name: "Saving…" }).matches(":disabled"),
  ).toBe(true);
  fireEvent.keyDown(screen.getByLabelText("Title"), {
    key: "s",
    metaKey: true,
  });
  fireEvent.change(screen.getByLabelText("Search credentials"), {
    target: { value: "latest query" },
  });
  await act(async () =>
    finish({
      ...context,
      operation: "save",
      state: "saved",
      credential_id: "assigned",
    }),
  );
  await screen.findByText("Committed details");
  expect(
    (screen.getByLabelText("Search credentials") as HTMLInputElement).value,
  ).toBe("latest query");
  expect(
    (window.__TAURI__!.core.invoke as any).mock.calls.filter(
      ([command]: [string]) => command === "create_credential",
    ),
  ).toHaveLength(1);
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "search_credentials",
    { ...context, query: "latest query", cursor: null },
  );
});
test("dirty dialog traps focus and restores the triggering field on Escape", async () => {
  mount((c) =>
    c === "list_credentials" ? { ...scope, summaries: [], next: null } : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  const title = screen.getByLabelText("Title");
  fireEvent.change(title, { target: { value: "Retain me" } });
  title.focus();
  fireEvent.keyDown(title, { key: "n", metaKey: true });
  const dialog = screen.getByRole("dialog");
  const cancel = screen.getAllByRole("button", { name: "Cancel" }).at(-1)!;
  expect(document.activeElement).toBe(cancel);
  fireEvent.keyDown(cancel, { key: "Tab" });
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Save changes" }),
  );
  fireEvent.keyDown(document.activeElement!, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(cancel);
  fireEvent.keyDown(dialog, { key: "Escape" });
  expect(document.activeElement).toBe(title);
  expect((title as HTMLInputElement).value).toBe("Retain me");
});
test("saving dirty dialog keeps keyboard focus contained while controls are disabled", async () => {
  let finish!: (v: unknown) => void;
  mount((c) =>
    c === "list_credentials"
      ? { ...scope, summaries: [], next: null }
      : c === "create_credential"
        ? new Promise((r) => (finish = r))
        : {},
  );
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "Save me" },
  });
  fireEvent.keyDown(screen.getByLabelText("Title"), {
    key: "n",
    metaKey: true,
  });
  fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
  expect(fireEvent.keyDown(screen.getByRole("dialog"), { key: "Tab" })).toBe(
    false,
  );
  await act(async () =>
    finish({
      ...context,
      operation: "save",
      state: "failed",
      credential_id: null,
    }),
  );
  expect((screen.getByLabelText("Title") as HTMLInputElement).value).toBe(
    "Save me",
  );
});
