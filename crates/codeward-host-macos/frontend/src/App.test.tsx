// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { App } from "./App";
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete window.__TAURI__;
});
function native(
  handler: (command: string, body: Record<string, unknown>) => unknown,
  existing = false,
) {
  let firstOpening = true;
  window.__TAURI__ = {
    core: {
      invoke: vi.fn(async (command, body = {}) => {
        if (command === "open_managed_vault" && firstOpening) {
          firstOpening = false;
          return { epoch: 100, revocation: 0, state: existing ? "locked" : "missing" };
        }
        return handler(command, body);
      }) as never,
    },
  };
}
test("welcome retains theme and setup validation", () => {
  render(<App />);
  fireEvent.click(screen.getByLabelText("Switch to light mode"));
  expect(document.documentElement.dataset.theme).toBe("light");
  fireEvent.click(screen.getByText("Create your vault"));
  fireEvent.submit(
    screen.getByRole("button", { name: "Continue" }).closest("form")!,
  );
  expect(screen.getByRole("alert").textContent).toContain(
    "Enter a master password",
  );
});
test("late opening is canceled after navigation", async () => {
  let done!: (x: unknown) => void;
  native((c) =>
    c === "open_managed_vault" ? new Promise((r) => (done = r)) : {},
    true,
  );
  render(<App />);
  fireEvent.click(await screen.findByText("Unlock Vault"));
  fireEvent.click(screen.getByText("Back"));
  done({ epoch: 2, revocation: 0, state: "locked" });
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "cancel_vault_setup",
      { epoch: 2 },
    ),
  );
  expect(screen.getByRole("heading", { name: "Codeward" })).toBeTruthy();
});
test("unlock dispatch clears password and wrong password allows retry", async () => {
  native((c) =>
    c === "open_managed_vault"
      ? { epoch: 1, revocation: 0, state: "locked" }
      : { epoch: 1, revocation: 0, state: "locked", error: "wrong_password" },
    true,
  );
  render(<App />);
  fireEvent.click(await screen.findByText("Unlock Vault"));
  await screen.findByLabelText("Master password");
  fireEvent.change(screen.getByLabelText("Master password"), {
    target: { value: "synthetic" },
  });
  fireEvent.submit(
    screen.getByRole("button", { name: "Unlock" }).closest("form")!,
  );
  await screen.findByText("Incorrect password. Try again.");
  expect(
    (screen.getByLabelText("Master password") as HTMLInputElement).value,
  ).toBe("");
});
async function setupForm() {
  fireEvent.click(await screen.findByRole("button", { name: "Create your vault" }));
  fireEvent.change(screen.getByLabelText("Master password"), {
    target: { value: "synthetic master" },
  });
  fireEvent.change(screen.getByLabelText("Confirm password"), {
    target: { value: "synthetic master" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}
function deferred() {
  let resolve!: (v: unknown) => void, reject!: (e: unknown) => void;
  const promise = new Promise((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
test("connection loss invalidates outstanding recovery code", async () => {
  const code = deferred();
  native((c) =>
    c === "begin_vault_setup"
      ? { epoch: 1, revocation: 0, state: "recovery" }
      : c === "get_setup_recovery_code"
        ? code.promise
        : c === "get_vault_setup_state"
          ? Promise.reject("lost")
          : {},
  );
  render(<App />);
  await setupForm();
  await screen.findByText("Keep a way back in");
  await screen.findByText("Connection lost");
  code.resolve({ epoch: 1, code: "SYNTHETIC CODE" });
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "cancel_vault_setup",
      { epoch: 1 },
    ),
  );
  expect(screen.queryByText("SYNTHETIC CODE")).toBeNull();
});
test("cancel setup discards a late opening and cancels its native epoch", async () => {
  const opening = deferred();
  native((c) => (c === "begin_vault_setup" ? opening.promise : {}));
  render(<App />);
  await setupForm();
  fireEvent.click(screen.getByText("Back"));
  opening.resolve({ epoch: 4, revocation: 0, state: "recovery" });
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "cancel_vault_setup",
      { epoch: 4 },
    ),
  );
  expect(screen.queryByText("Keep a way back in")).toBeNull();
});
test("navigation clears exposed code and ignores pending status", async () => {
  const polling = deferred();
  native((c) =>
    c === "begin_vault_setup"
      ? { epoch: 2, revocation: 0, state: "recovery" }
      : c === "get_setup_recovery_code"
        ? { epoch: 2, code: "SYNTHETIC CODE" }
        : c === "get_vault_setup_state"
          ? polling.promise
          : {},
  );
  render(<App />);
  await setupForm();
  await screen.findByText("SYNTHETIC CODE");
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "get_vault_setup_state",
      {},
    ),
  );
  fireEvent.click(screen.getByText("Cancel setup"));
  polling.resolve({ epoch: 2, revocation: 0, state: "recovery" });
  expect(screen.queryByText("SYNTHETIC CODE")).toBeNull();
  expect(screen.getByRole("heading", { name: "Codeward" })).toBeTruthy();
});
test("saving and unknown enrollment never claim committed success", async () => {
  let state = "recovery";
  native((c) =>
    c === "begin_vault_setup"
      ? { epoch: 3, revocation: 0, state: "recovery" }
      : c === "get_setup_recovery_code"
        ? { epoch: 3, code: "SYNTHETIC CODE" }
        : c === "confirm_vault_setup"
          ? ((state = "unknown"), { epoch: 3, revocation: 0, state: "saving" })
          : { epoch: 3, revocation: 0, state },
  );
  render(<App />);
  await setupForm();
  await screen.findByText("SYNTHETIC CODE");
  fireEvent.click(screen.getByText("I've written it down"));
  expect(screen.queryByText("SYNTHETIC CODE")).toBeNull();
  fireEvent.change(screen.getByLabelText("Type your recovery code"), {
    target: { value: "SYNTHETIC CODE" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Create vault" }));
  await screen.findByText("Save not confirmed");
  expect(screen.queryByText("Vault saved")).toBeNull();
});
test("lost confirmation reply polls actual committed state", async () => {
  let state = "recovery";
  native((c) =>
    c === "begin_vault_setup"
      ? { epoch: 5, revocation: 0, state: "recovery" }
      : c === "get_setup_recovery_code"
        ? { epoch: 5, code: "SYNTHETIC CODE" }
        : c === "confirm_vault_setup"
          ? ((state = "created"), Promise.reject("lost"))
          : { epoch: 5, revocation: 0, state },
  );
  render(<App />);
  await setupForm();
  await screen.findByText("SYNTHETIC CODE");
  fireEvent.click(screen.getByText("I've written it down"));
  fireEvent.change(screen.getByLabelText("Type your recovery code"), {
    target: { value: "SYNTHETIC CODE" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Create vault" }));
  await screen.findByText("Vault saved");
});
test("navigation during authentication rejects a late unlocked reply", async () => {
  const attempt = deferred();
  native((c) =>
    c === "open_managed_vault"
      ? { epoch: 7, revocation: 0, state: "locked" }
      : c === "unlock_managed_vault"
        ? attempt.promise
        : {},
    true,
  );
  render(<App />);
  fireEvent.click(await screen.findByText("Unlock Vault"));
  await screen.findByLabelText("Master password");
  fireEvent.change(screen.getByLabelText("Master password"), {
    target: { value: "synthetic" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
  fireEvent.click(screen.getByText("Back"));
  attempt.resolve({ epoch: 7, revocation: 0, state: "unlocked" });
  expect(screen.getByRole("heading", { name: "Codeward" })).toBeTruthy();
  expect(screen.queryByText("Your vault")).toBeNull();
});
test("missing vault does not start creation", async () => {
  native(() => ({ epoch: 8, revocation: 0, state: "missing" }), true);
  render(<App />);
  fireEvent.click(await screen.findByText("Unlock Vault"));
  await screen.findByText("No vault found");
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith(
    "begin_vault_setup",
    expect.anything(),
  );
});
test("revocation clears typed password even while already locked", async () => {
  let revocation = 0;
  native((c) =>
    c === "open_managed_vault"
      ? { epoch: 9, revocation: 0, state: "locked" }
      : { epoch: 9, revocation, state: "locked" },
    true,
  );
  render(<App />);
  fireEvent.click(await screen.findByText("Unlock Vault"));
  await screen.findByLabelText("Master password");
  fireEvent.change(screen.getByLabelText("Master password"), {
    target: { value: "synthetic pre-auth" },
  });
  revocation = 1;
  await waitFor(() =>
    expect(
      (screen.getByLabelText("Master password") as HTMLInputElement).value,
    ).toBe(""),
  );
});
test("native security lock clears the credential editor and dirty dialog immediately", async () => {
  let state = "locked";
  native((c) =>
    c === "open_managed_vault"
      ? { epoch: 1, revocation: 0, state }
      : c === "unlock_managed_vault"
        ? ((state = "unlocked"), { epoch: 1, revocation: 0, state })
        : c === "list_credentials"
          ? {
              epoch: 1,
              revocation: 0,
              generation: "1",
              revision: "1",
              summaries: [],
              next: null,
            }
          : { epoch: 1, revocation: state === "locked" ? 1 : 0, state },
    true,
  );
  render(<App />);
  fireEvent.click(await screen.findByText("Unlock Vault"));
  await screen.findByLabelText("Master password");
  fireEvent.change(screen.getByLabelText("Master password"), {
    target: { value: "synthetic" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
  await screen.findByText("Your vault is ready");
  fireEvent.click(screen.getByRole("button", { name: "Add credential" }));
  fireEvent.change(screen.getByLabelText("Title"), {
    target: { value: "SENSITIVE EDIT" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await screen.findByRole("dialog");
  state = "locked";
  await screen.findByRole("heading", { name: "Unlock your vault" });
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(screen.queryByDisplayValue("SENSITIVE EDIT")).toBeNull();
});

test("a saved vault replaces creation with Unlock Vault at home and on restart", async () => {
  let exists = false;
  let epoch = 0;
  let state = "idle";
  window.__TAURI__ = {
    core: {
      invoke: vi.fn(async (command: string) => {
        if (command === "open_managed_vault") {
          epoch++;
          state = exists ? "locked" : "missing";
        }
        if (command === "begin_vault_setup") {
          epoch++;
          state = "recovery";
        }
        if (command === "get_setup_recovery_code")
          return { epoch, code: "SYNTHETIC CODE" };
        if (command === "confirm_vault_setup") {
          exists = true;
          state = "created";
        }
        if (command === "cancel_vault_setup") state = "locked";
        return { epoch, revocation: 0, state };
      }) as never,
    },
  };
  const first = render(<App />);
  await setupForm();
  await screen.findByText("SYNTHETIC CODE");
  fireEvent.click(screen.getByText("I've written it down"));
  fireEvent.change(screen.getByLabelText("Type your recovery code"), {
    target: { value: "SYNTHETIC CODE" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Create vault" }));
  await screen.findByRole("heading", { name: "Vault saved" });
  fireEvent.click(screen.getByRole("button", { name: "Back to Home" }));
  await screen.findByRole("button", { name: "Unlock Vault" });
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
  first.unmount();
  render(<App />);
  await screen.findByRole("button", { name: "Unlock Vault" });
  expect(screen.getByRole("heading", { name: "Codeward" })).toBeTruthy();
  expect(screen.queryByLabelText("Master password")).toBeNull();
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});
