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
test("options start collapsed and opening them does not generate a password", () => {
  const { onDraft } = mount(() => draft);
  const disclosure = screen.getByRole("button", { name: "Options" });
  expect(disclosure.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByRole("spinbutton", { name: "Password length" })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: "Symbols" })).toBeNull();
  expect(screen.getByText("24 characters · Uppercase, lowercase, digits, symbols")).toBeTruthy();
  fireEvent.click(disclosure);
  expect(disclosure.getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByRole("spinbutton", { name: "Password length" })).toBeTruthy();
  expect(screen.getByRole("checkbox", { name: "Symbols" })).toBeTruthy();
  expect(onDraft).not.toHaveBeenCalled();
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalled();
});

test("option edits leave the generated selection unchanged until explicit regeneration", async () => {
  let count = 0;
  const { onDraft } = mount((c) => c === "generate_password_draft" ? { ...draft, draft: `opaque-${++count}` } : {});
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  fireEvent.change(screen.getByRole("spinbutton", { name: "Password length" }), { target: { value: "32" } });
  fireEvent.click(screen.getByRole("checkbox", { name: "Symbols" }));
  expect(screen.getByText("32 characters · Uppercase, lowercase, digits")).toBeTruthy();
  expect(onDraft).toHaveBeenCalledTimes(1);
  expect(count).toBe(1);
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  expect(screen.queryByRole("spinbutton", { name: "Password length" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Regenerate password" }));
  await waitFor(() => expect(onDraft).toHaveBeenLastCalledWith({ ...draft, draft: "opaque-2" }));
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith("generate_password_draft", {
    ...scope,
    options: { length: 32, uppercase: true, lowercase: true, digits: true, symbols: false },
  });
});

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
test("Escape masks the generated preview while keeping its draft and options", async () => {
  const { onDraft } = mount((c) =>
    c === "generate_password_draft"
      ? draft
      : { ...draft, password: "ESCAPE-PREVIEW", expires_in_ms: 30000 },
  );
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  fireEvent.change(screen.getByRole("spinbutton", { name: "Password length" }), {
    target: { value: "32" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Show generated password" }));
  await screen.findByText("ESCAPE-PREVIEW");
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByText("ESCAPE-PREVIEW")).toBeNull();
  expect(screen.getByRole("button", { name: "Show generated password" })).toBeTruthy();
  expect(screen.getByRole("button", { name: "Regenerate password" })).toBeTruthy();
  expect((screen.getByRole("spinbutton", { name: "Password length" }) as HTMLInputElement).value).toBe("32");
  expect(onDraft).toHaveBeenCalledTimes(1);
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalledWith("discard_password_draft", draft);
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
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
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

test("length stays empty while editing and replacement digits generate the requested length", async () => {
  mount(() => draft);
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  const length = screen.getByLabelText("Password length") as HTMLInputElement;
  const generate = screen.getByRole("button", {
    name: "Generate password",
  }) as HTMLButtonElement;

  fireEvent.focus(length);
  fireEvent.change(length, { target: { value: "" } });
  expect(length.value).toBe("");
  expect(generate.disabled).toBe(true);
  fireEvent.click(generate);
  expect(window.__TAURI__!.core.invoke).not.toHaveBeenCalled();

  fireEvent.change(length, { target: { value: "1" } });
  expect(length.value).toBe("1");
  expect(generate.disabled).toBe(true);
  fireEvent.change(length, { target: { value: `${length.value}2` } });
  expect(length.value).toBe("12");
  expect(generate.disabled).toBe(false);
  fireEvent.click(generate);
  await screen.findByText("Generated password ready");
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "generate_password_draft",
    {
      ...scope,
      options: {
        length: 12,
        uppercase: true,
        lowercase: true,
        digits: true,
        symbols: true,
      },
    },
  );
});

test("leaving an empty length displays zero and keeps generation disabled", () => {
  mount(() => draft);
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  const length = screen.getByLabelText("Password length") as HTMLInputElement;
  fireEvent.focus(length);
  fireEvent.change(length, { target: { value: "" } });
  expect(length.value).toBe("");
  fireEvent.blur(length);
  expect(length.value).toBe("0");
  expect(
    (screen.getByRole("button", { name: "Generate password" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});

test("refocusing an empty length's zero allows typing a fresh number", () => {
  mount(() => draft);
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  const length = screen.getByLabelText("Password length") as HTMLInputElement;
  fireEvent.focus(length);
  fireEvent.change(length, { target: { value: "" } });
  fireEvent.blur(length);
  expect(length.value).toBe("0");

  fireEvent.focus(length);
  expect(length.value).toBe("");
  fireEvent.change(length, { target: { value: `${length.value}1` } });
  fireEvent.change(length, { target: { value: `${length.value}2` } });
  expect(length.value).toBe("12");
  expect(
    (screen.getByRole("button", { name: "Generate password" }) as HTMLButtonElement)
      .disabled,
  ).toBe(false);
});

test("leaving a length removes leading zeros without changing its numeric value", async () => {
  mount(() => draft);
  fireEvent.click(screen.getByRole("button", { name: "Options" }));
  const length = screen.getByLabelText("Password length") as HTMLInputElement;
  fireEvent.focus(length);
  fireEvent.change(length, { target: { value: "0012" } });
  fireEvent.blur(length);
  expect(length.value).toBe("12");
  fireEvent.click(screen.getByRole("button", { name: "Generate password" }));
  await screen.findByText("Generated password ready");
  expect(window.__TAURI__!.core.invoke).toHaveBeenCalledWith(
    "generate_password_draft",
    expect.objectContaining({
      options: expect.objectContaining({ length: 12 }),
    }),
  );
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
