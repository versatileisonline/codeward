const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

const script = fs.readFileSync(path.join(__dirname, "../desktop-assets/probe.js"), "utf8");
const flush = () => new Promise((resolve) => setImmediate(resolve));

function renderer() {
  const elements = new Map(["state", "preview", "draft"].map((id) => [
    `#${id}`,
    {
      textContent: "",
      value: "",
      hidden: true,
      getClientRects() { return this.hidden ? [] : [{}]; },
    },
  ]));
  const calls = [];
  let tick;
  const window = {
    __TAURI__: { core: { invoke(command) {
      return new Promise((resolve, reject) => calls.push({ command, resolve, reject }));
    } } },
    setInterval(callback) { tick = callback; },
  };
  vm.runInNewContext(script, {
    document: { visibilityState: "visible", querySelector: (id) => elements.get(id) },
    window,
  });
  function call(command) {
    const next = calls.shift();
    assert.equal(next?.command, command);
    return next;
  }
  const preview = elements.get("#preview");
  const draft = elements.get("#draft");
  function assertCleared() {
    assert.equal(preview.textContent, "");
    assert.equal(draft.value, "");
    assert.equal(preview.hidden, true);
    assert.equal(draft.hidden, true);
  }
  async function arm() {
    call("get_session_state").resolve("enrollment");
    await flush();
    assert.match(preview.textContent, /^Synthetic /);
    assert.match(draft.value, /^Synthetic /);
    assert.equal(preview.hidden, false);
    assert.equal(draft.hidden, false);
    call("probe_visible").resolve();
    await flush();
  }
  return { arm, assertCleared, call, calls, tick: () => tick() };
}

test("locked status clears both fields before acknowledging and stops polling", async () => {
  const view = renderer();
  await view.arm();
  view.tick();
  view.call("get_session_state").resolve("locked");
  await flush();
  view.assertCleared();
  view.call("probe_cleared").resolve();
  await flush();
  view.tick();
  assert.equal(view.calls.length, 0);
});

test("IPC failure clears fields and a late enrollment status cannot repopulate them", async () => {
  const view = renderer();
  await view.arm();
  view.tick();
  view.call("get_session_state").reject(new Error("unavailable"));
  await flush();
  view.assertCleared();
  view.tick();
  view.call("get_session_state").resolve("enrollment");
  await flush();
  view.assertCleared();
  assert.equal(view.calls.length, 0);
});

test("source unavailable clears fields without claiming native lock", async () => {
  const view = renderer();
  await view.arm();
  view.tick();
  view.call("get_session_state").resolve("unavailable");
  await flush();
  view.assertCleared();
  assert.equal(view.calls.length, 0);
});
