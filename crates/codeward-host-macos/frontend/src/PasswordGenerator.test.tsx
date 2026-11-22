// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { PasswordGenerator } from "./PasswordGenerator";
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  delete window.__TAURI__;
});
const scope = { epoch: 1, revocation: 0, generation: "2", revision: "3" };
const draft = { ...scope, draft: "opaque-1" };
function mount(handler: (c: string, b: Record<string, unknown>) => unknown) {
  window.__TAURI__ = {
    core: { invoke: vi.fn(async (c, b = {}) => handler(c, b)) as never },
  };
  const onDraft = vi.fn();
  const result = render(
    <PasswordGenerator
      scope={scope}
      onDraft={onDraft}
      onBusy={() => {}}
      disabled={false}
    />,
  );
  return { ...result, onDraft };
}
test("generation sends options and retains only opaque reference, preview is explicit", async () => {
  const { onDraft } = mount((c) =>
    c === "generate_password_draft"
      ? draft
      : { ...draft, password: "SYNTHETIC-GENERATED", expires_in_ms: 30000 },
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  expect(onDraft).toHaveBeenCalledWith(draft);
  expect(screen.queryByText("SYNTHETIC-GENERATED")).toBeNull();
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "generate_password_draft",
    {
      ...scope,
      options: {
        length: 24,
        uppercase: true,
        lowercase: true,
        digits: true,
        symbols: true,
      },
    },
  );
  fireEvent.click(
    screen.getByRole("button", { name: "Show generated password" }),
  );
  await screen.findByText("SYNTHETIC-GENERATED");
  fireEvent.blur(window);
  expect(screen.queryByText("SYNTHETIC-GENERATED")).toBeNull();
});
test("navigation discards a late generated draft without selecting it", async () => {
  let done!: (x: unknown) => void;
  const { unmount, onDraft } = mount((c) =>
    c === "generate_password_draft" ? new Promise((r) => (done = r)) : {},
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  unmount();
  done(draft);
  await waitFor(() =>
    expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
      "discard_password_draft",
      draft,
    ),
  );
  expect(onDraft).not.toHaveBeenCalled();
});
test("late reveal after blur is discarded and reveal lifetime expires", async () => {
  let done!: (x: unknown) => void;
  mount((c) =>
    c === "generate_password_draft" ? draft : new Promise((r) => (done = r)),
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  fireEvent.click(
    screen.getByRole("button", { name: "Show generated password" }),
  );
  fireEvent.blur(window);
  done({ ...draft, password: "LATE-SECRET", expires_in_ms: 30000 });
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "Show generated password" }),
    ).toBeTruthy(),
  );
  expect(screen.queryByText("LATE-SECRET")).toBeNull();
});
test("failed regeneration keeps old reference and manual switch discards it", async () => {
  let count = 0;
  const { onDraft } = mount((c) => {
    if (c === "generate_password_draft") {
      if (count++) throw "invalid_input";
      return draft;
    }
    return draft;
  });
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  fireEvent.click(screen.getByRole("button", { name: "Regenerate password" }));
  await screen.findByText(/Could not generate/);
  expect(onDraft).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "Use typed password" }));
  expect(onDraft).toHaveBeenLastCalledWith(null);
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "discard_password_draft",
    draft,
  );
});
test("invalid options cannot dispatch generation", async () => {
  mount(() => draft);
  fireEvent.change(screen.getByLabelText("Password length"), {
    target: { value: "11" },
  });
  expect(
    (
      screen.getByRole("button", {
        name: "Generate password",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
  fireEvent.change(screen.getByLabelText("Password length"), {
    target: { value: "24" },
  });
  for (const name of ["Uppercase", "Lowercase", "Digits", "Symbols"])
    fireEvent.click(screen.getByLabelText(name));
  expect(
    (
      screen.getByRole("button", {
        name: "Generate password",
      }) as HTMLButtonElement
    ).disabled,
  ).toBe(true);
});

test("preview expires on its returned lifetime and old reveal cannot replace regeneration", async () => {
  let revealCount = 0,
    resolveOld!: (x: unknown) => void,
    genCount = 0;
  mount((c) => {
    if (c === "generate_password_draft")
      return { ...draft, draft: `opaque-${++genCount}` };
    if (c === "reveal_password_draft") {
      if (revealCount++ === 0) return new Promise((r) => (resolveOld = r));
      return {
        ...draft,
        draft: "opaque-2",
        password: "SHORT-LIVED",
        expires_in_ms: 40,
      };
    }
    return {};
  });
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  fireEvent.click(
    screen.getByRole("button", { name: "Show generated password" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Regenerate password" }));
  await waitFor(() =>
    expect(
      (
        screen.getByRole("button", {
          name: "Regenerate password",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false),
  );
  resolveOld({ ...draft, password: "STALE-REVEAL", expires_in_ms: 30000 });
  fireEvent.click(
    screen.getByRole("button", { name: "Show generated password" }),
  );
  await screen.findByText("SHORT-LIVED");
  await waitFor(() => expect(screen.queryByText("SHORT-LIVED")).toBeNull());
  expect(screen.queryByText("STALE-REVEAL")).toBeNull();
});
