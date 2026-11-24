// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Welcome } from "./Welcome";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete window.__TAURI__;
});

function native(handler: (command: string) => unknown) {
  const invoke = vi.fn(async (command: string) => handler(command));
  window.__TAURI__ = { core: { invoke: invoke as never } };
  return invoke;
}

test("a missing vault offers only explicit creation after the native check", async () => {
  const invoke = native(() => ({ epoch: 1, revocation: 0, state: "missing" }));
  const create = vi.fn();
  render(<Welcome create={create} open={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Create your vault" }));
  expect(create).toHaveBeenCalledOnce();
  expect(screen.queryByRole("button", { name: "Unlock Vault" })).toBeNull();
  expect(invoke).toHaveBeenCalledWith("cancel_vault_setup", { epoch: 1 });
  expect(invoke).not.toHaveBeenCalledWith("begin_vault_setup", expect.anything());
});

test("an existing vault keeps the landing page with only Unlock Vault", async () => {
  native(() => ({ epoch: 2, revocation: 0, state: "locked" }));
  const open = vi.fn();
  render(<Welcome create={vi.fn()} open={open} />);
  expect(screen.getByRole("heading", { name: "Codeward" })).toBeTruthy();
  expect(open).not.toHaveBeenCalled();
  fireEvent.click(await screen.findByRole("button", { name: "Unlock Vault" }));
  expect(open).toHaveBeenCalledOnce();
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});

test("opening is polled before enabling the single cover action", async () => {
  native((command) => ({ epoch: 3, revocation: 0, state: command === "open_managed_vault" ? "opening" : "locked" }));
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  expect((screen.getByRole("button", { name: "Checking vault…" }) as HTMLButtonElement).disabled).toBe(true);
  await screen.findByRole("button", { name: "Unlock Vault" });
});

test("failed storage checks offer retry without suggesting a new vault", async () => {
  const invoke = native(() => ({ epoch: 4, revocation: 0, state: "failed", error: "storage" }));
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  fireEvent.click(await screen.findByRole("button", { name: "Try again" }));
  await waitFor(() => expect(invoke.mock.calls.filter(([c]) => c === "open_managed_vault")).toHaveLength(2));
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});

test("a late check is canceled after leaving the cover", async () => {
  let done!: (value: unknown) => void;
  const invoke = native((c) => c === "open_managed_vault" ? new Promise((r) => { done = r; }) : {});
  const { unmount } = render(<Welcome create={vi.fn()} open={vi.fn()} />);
  unmount();
  done({ epoch: 5, revocation: 0, state: "opening" });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("cancel_vault_setup", { epoch: 5 }));
});

test("returning home checks native state again after vault creation", async () => {
  let state = "missing";
  native(() => ({ epoch: 6, revocation: 0, state }));
  const first = render(<Welcome create={vi.fn()} open={vi.fn()} />);
  await screen.findByRole("button", { name: "Create your vault" });
  first.unmount();
  state = "locked";
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  await screen.findByRole("button", { name: "Unlock Vault" });
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});

test("a lost connection does not turn into permission to create", async () => {
  native(() => Promise.reject("unavailable"));
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  await screen.findByRole("button", { name: "Try again" });
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});

test("an interrupted opening cannot be mistaken for an existing vault", async () => {
  native((command) => ({
    epoch: 7,
    revocation: command === "open_managed_vault" ? 0 : 1,
    state: command === "open_managed_vault" ? "opening" : "locked",
  }));
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  await screen.findByRole("button", { name: "Try again" });
  expect(screen.queryByRole("button", { name: "Unlock Vault" })).toBeNull();
});

test("a stale poll cannot select an action or cancel a newer epoch", async () => {
  const invoke = native((command) => ({
    epoch: command === "open_managed_vault" ? 8 : 9,
    revocation: 0,
    state: command === "open_managed_vault" ? "opening" : "missing",
  }));
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  await screen.findByRole("button", { name: "Try again" });
  expect(invoke).toHaveBeenCalledWith("cancel_vault_setup", { epoch: 8 });
  expect(invoke).not.toHaveBeenCalledWith("cancel_vault_setup", { epoch: 9 });
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});

test("the cover waits for native release before allowing another opening", async () => {
  native((command) => command === "cancel_vault_setup"
    ? Promise.reject("unavailable")
    : { epoch: 10, revocation: 0, state: "locked" });
  render(<Welcome create={vi.fn()} open={vi.fn()} />);
  await screen.findByRole("button", { name: "Try again" });
  expect(screen.queryByRole("button", { name: "Unlock Vault" })).toBeNull();
});

test.each(["recovery_saving", "recovery_unknown", "recovery_checking", "recovery_saved", "recovery_failed"])("%s resumes an existing vault without cancelling the pending recovery", async (state) => {
  const invoke = native(() => ({ epoch: 11, revocation: 0, state, error: "storage" }));
  const open = vi.fn();
  render(<Welcome create={vi.fn()} open={open} />);
  fireEvent.click(await screen.findByRole("button", { name: "Unlock Vault" }));
  expect(open).toHaveBeenCalledOnce();
  expect(invoke).not.toHaveBeenCalledWith("cancel_vault_setup", expect.anything());
  expect(screen.queryByRole("button", { name: "Create your vault" })).toBeNull();
});
