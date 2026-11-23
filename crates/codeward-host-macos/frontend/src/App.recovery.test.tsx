// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { App } from "./App";
import type { Status } from "./native";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete window.__TAURI__;
});
function deferred() {
  let resolve!: (value: Status) => void;
  const promise = new Promise<Status>((done) => { resolve = done; });
  return { promise, resolve };
}
function vault(overrides?: (command: string, body: Record<string, unknown>) => unknown) {
  let status: Status = { epoch: 20, revocation: 0, state: "locked" };
  const invoke = vi.fn(async (command: string, body: Record<string, unknown> = {}) => {
    const custom = overrides?.(command, body);
    if (custom !== undefined) return custom;
    if (command === "begin_vault_recovery") status = { ...status, state: "recovery_ready", error: undefined };
    if (command === "complete_vault_recovery") status = { ...status, state: "recovery_saved", error: undefined };
    if (command === "lock_managed_vault") status = { ...status, state: "locked", revocation: status.revocation + 1, error: undefined };
    return status;
  });
  window.__TAURI__ = { core: { invoke: invoke as never } };
  return { invoke, set: (next: Partial<Status>) => { status = { ...status, ...next }; } };
}
async function recover() {
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Unlock Vault" }));
  fireEvent.click(await screen.findByRole("button", { name: "Forgot password?" }));
  await screen.findByRole("heading", { name: "Recover your vault" });
}
function paper(value = "SYNTHETIC RECOVERY") {
  fireEvent.change(screen.getByLabelText("Recovery code"), { target: { value } });
  fireEvent.click(screen.getByRole("button", { name: "Continue" }));
}
async function passwords(confirmation = "new synthetic password") {
  await screen.findByRole("heading", { name: "Choose a new password" });
  fireEvent.change(screen.getByLabelText("New master password"), { target: { value: "new synthetic password" } });
  fireEvent.change(screen.getByLabelText("Confirm password"), { target: { value: confirmation } });
  fireEvent.click(screen.getByRole("button", { name: "Save new password" }));
}
test("recovery authenticates paper before saving, preserves recovery code and requires ordinary unlock", async () => {
  const { invoke } = vault();
  await recover();
  expect((screen.getByLabelText("Recovery code") as HTMLInputElement).type).toBe("password");
  fireEvent.click(screen.getByRole("button", { name: "Show recovery code" }));
  expect((screen.getByLabelText("Recovery code") as HTMLInputElement).type).toBe("text");
  paper();
  await passwords();
  await screen.findByText("Your new master password is saved. Your existing recovery code still works.");
  expect(invoke).toHaveBeenCalledWith("begin_vault_recovery", { epoch: 20, revocation: 0, paper: "SYNTHETIC RECOVERY" });
  expect(invoke).toHaveBeenCalledWith("complete_vault_recovery", { epoch: 20, revocation: 0, password: "new synthetic password", confirmation: "new synthetic password" });
  expect(invoke.mock.calls.some(([command]) => command === "get_setup_recovery_code")).toBe(false);
  expect(screen.queryByLabelText("Recovery code")).toBeNull();
  expect(screen.queryByLabelText("New master password")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Back to unlock" }));
  await screen.findByRole("heading", { name: "Unlock your vault" });
  expect((screen.getByLabelText("Master password") as HTMLInputElement).value).toBe("");
  expect(screen.queryByText("Your vault is ready")).toBeNull();
});
test("wrong recovery code stays editable across locked polling without revealing or retaining the code", async () => {
  const { set } = vault((command) => command === "begin_vault_recovery" ? { epoch: 20, revocation: 0, state: "locked", error: "invalid_code" } : undefined);
  await recover();
  paper("WRONG SYNTHETIC CODE");
  await screen.findByText("The code doesn't match this vault. Try again.");
  set({ state: "locked", error: "invalid_code" });
  await new Promise((done) => setTimeout(done, 300));
  expect(screen.getByRole("heading", { name: "Recover your vault" })).toBeTruthy();
  expect((screen.getByLabelText("Recovery code") as HTMLInputElement).value).toBe("");
});
test("password mismatch clears both fields and never writes", async () => {
  const { invoke } = vault();
  await recover();
  paper();
  await passwords("different");
  await screen.findByText("Your passwords do not match.");
  expect((screen.getByLabelText("New master password") as HTMLInputElement).value).toBe("");
  expect((screen.getByLabelText("Confirm password") as HTMLInputElement).value).toBe("");
  expect(invoke.mock.calls.some(([command]) => command === "complete_vault_recovery")).toBe(false);
});
test("cancel recovery rejects a late paper reply and returns to a fresh locked form", async () => {
  const attempt = deferred();
  const { invoke } = vault((command) => command === "begin_vault_recovery" ? attempt.promise : undefined);
  await recover();
  paper();
  fireEvent.click(screen.getByRole("button", { name: "Back to unlock" }));
  await screen.findByRole("heading", { name: "Unlock your vault" });
  attempt.resolve({ epoch: 20, revocation: 0, state: "recovery_ready" });
  await waitFor(() => expect(screen.queryByRole("heading", { name: "Choose a new password" })).toBeNull());
  expect(invoke.mock.calls.filter(([command]) => command === "begin_vault_recovery")).toHaveLength(1);
});
test("native revocation discards recovery password fields immediately", async () => {
  const { set } = vault();
  await recover();
  paper();
  await screen.findByRole("heading", { name: "Choose a new password" });
  fireEvent.change(screen.getByLabelText("New master password"), { target: { value: "sensitive draft" } });
  set({ state: "locked", revocation: 1 });
  await screen.findByRole("heading", { name: "Unlock your vault" });
  expect(screen.queryByDisplayValue("sensitive draft")).toBeNull();
});
test("uncertain save requires fresh paper reconciliation and cannot retry a write", async () => {
  const { invoke, set } = vault((command) => {
    if (command === "complete_vault_recovery") { set({ state: "recovery_unknown" }); return { epoch: 20, revocation: 0, state: "recovery_unknown" }; }
    if (command === "reconcile_vault_recovery") { set({ state: "recovery_saved" }); return { epoch: 20, revocation: 0, state: "recovery_saved" }; }
    return undefined;
  });
  await recover();
  paper();
  await passwords();
  await screen.findByRole("heading", { name: "Save not confirmed" });
  expect(screen.getByText(/Keep your recovery code/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Back to unlock" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Check saved state" }));
  await screen.findByLabelText("Recovery code");
  await new Promise((done) => setTimeout(done, 300));
  paper("FRESH SYNTHETIC CODE");
  await screen.findByText("Your new master password is saved. Your existing recovery code still works.");
  expect(invoke).toHaveBeenCalledWith("reconcile_vault_recovery", { epoch: 20, revocation: 0, paper: "FRESH SYNTHETIC CODE" });
  expect(invoke.mock.calls.filter(([command]) => command === "complete_vault_recovery")).toHaveLength(1);
});
test("a definite save failure requires paper again before a fresh password attempt", async () => {
  const { set } = vault((command) => {
    if (command === "complete_vault_recovery") { set({ state: "recovery_failed", error: "storage" }); return { epoch: 20, revocation: 0, state: "recovery_failed", error: "storage" }; }
    return undefined;
  });
  await recover();
  paper();
  await passwords();
  await screen.findByRole("heading", { name: "Password wasn't saved" });
  fireEvent.click(screen.getByRole("button", { name: "Try recovery again" }));
  await screen.findByRole("heading", { name: "Recover your vault" });
  expect(screen.queryByLabelText("New master password")).toBeNull();
  paper();
  await screen.findByRole("heading", { name: "Choose a new password" });
});

test("checking an uncertain save cannot be abandoned into a stuck or writable state", async () => {
  const check = deferred();
  const { set } = vault((command) => {
    if (command === "complete_vault_recovery") { set({ state: "recovery_unknown" }); return { epoch: 20, revocation: 0, state: "recovery_unknown" }; }
    if (command === "reconcile_vault_recovery") return check.promise;
    return undefined;
  });
  await recover();
  paper();
  await passwords();
  fireEvent.click(await screen.findByRole("button", { name: "Check saved state" }));
  paper();
  expect((screen.getByRole("button", { name: "Back" }) as HTMLButtonElement).disabled).toBe(true);
  check.resolve({ epoch: 20, revocation: 1, state: "recovery_saved" });
  await screen.findByText("Your new master password is saved. Your existing recovery code still works.");
});

test("oversized paper input gives a retryable code error without dispatch", async () => {
  const { invoke } = vault();
  await recover();
  paper("x".repeat(257));
  await screen.findByText("The code doesn't match this vault. Try again.");
  expect((screen.getByLabelText("Recovery code") as HTMLInputElement).value).toBe("");
  expect(invoke.mock.calls.some(([command]) => command === "begin_vault_recovery")).toBe(false);
});

test.each([
  ["recovery_saving", "Saving new password…"],
  ["recovery_unknown", "Save not confirmed"],
  ["recovery_saved", "New password saved"],
  ["recovery_failed", "Password wasn't saved"],
])("reopening %s restores the authoritative recovery outcome", async (state, heading) => {
  const { set, invoke } = vault();
  set({ state });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Unlock Vault" }));
  await screen.findByRole("heading", { name: heading });
  expect(screen.queryByRole("button", { name: "Unlock" })).toBeNull();
  expect(screen.queryByLabelText("New master password")).toBeNull();
  expect(invoke.mock.calls.some(([command]) => command === "cancel_vault_setup")).toBe(false);
});

test("a lost write reply resolves by polling committed state without sending a second write", async () => {
  const { invoke, set } = vault((command) => {
    if (command === "complete_vault_recovery") {
      set({ state: "recovery_saved", revocation: 1 });
      return Promise.reject("lost");
    }
    return undefined;
  });
  await recover();
  paper();
  await passwords();
  await screen.findByText("Your new master password is saved. Your existing recovery code still works.");
  expect(invoke.mock.calls.filter(([command]) => command === "complete_vault_recovery")).toHaveLength(1);
});

test("duplicate form submissions dispatch a single recovery authentication and clear the code immediately", async () => {
  const attempt = deferred();
  const { invoke } = vault((command) => command === "begin_vault_recovery" ? attempt.promise : undefined);
  await recover();
  const form = screen.getByRole("button", { name: "Continue" }).closest("form")!;
  fireEvent.change(screen.getByLabelText("Recovery code"), { target: { value: "SYNTHETIC RECOVERY" } });
  fireEvent.submit(form);
  fireEvent.submit(form);
  expect((screen.getByLabelText("Recovery code") as HTMLInputElement).value).toBe("");
  expect(invoke.mock.calls.filter(([command]) => command === "begin_vault_recovery")).toHaveLength(1);
  attempt.resolve({ epoch: 20, revocation: 0, state: "recovery_ready" });
  await screen.findByRole("heading", { name: "Choose a new password" });
});

test("unavailable reconciliation requires restart without offering an impossible check", async () => {
  const { set } = vault();
  set({ state: "recovery_unknown", error: "unavailable" });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Unlock Vault" }));
  await screen.findByText("Codeward could not check the saved state. Keep your recovery code, quit and reopen Codeward, then use recovery again if needed.");
  expect(screen.queryByRole("button", { name: "Check saved state" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Back to unlock" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Save new password" })).toBeNull();
});

test("losing reconciliation capability clears the recheck form and presents restart instructions", async () => {
  const { set } = vault((command) => {
    if (command === "reconcile_vault_recovery") {
      set({ state: "recovery_unknown", error: "unavailable" });
      return { epoch: 20, revocation: 0, state: "recovery_unknown", error: "unavailable" };
    }
    return undefined;
  });
  set({ state: "recovery_unknown" });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: "Unlock Vault" }));
  fireEvent.click(await screen.findByRole("button", { name: "Check saved state" }));
  paper();
  await screen.findByText("Codeward could not check the saved state. Keep your recovery code, quit and reopen Codeward, then use recovery again if needed.");
  expect(screen.queryByLabelText("Recovery code")).toBeNull();
  expect(screen.queryByRole("button", { name: "Check saved state" })).toBeNull();
});
