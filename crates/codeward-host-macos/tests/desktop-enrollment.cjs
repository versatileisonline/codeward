const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");
const script = fs.readFileSync(path.join(__dirname, "../desktop-assets/welcome.js"), "utf8");
const flush = () => new Promise(setImmediate);

function renderer() {
  const elements = new Map();
  function element(selector) {
    if (!elements.has(selector)) elements.set(selector, {
      textContent: "", value: "", hidden: ["#setup", "#about", "#recovery", "#setup-result"].includes(selector),
      disabled: false, type: "password", dataset: {}, listeners: {}, attributes: {},
      addEventListener(name, fn) { this.listeners[name] = fn; },
      setAttribute(name, value) { this.attributes[name] = value; },
      removeAttribute(name) { delete this.attributes[name]; },
      focus() {},
      reset() { element("#master-password").value = ""; element("#confirm-password").value = ""; },
    });
    return elements.get(selector);
  }
  const calls = [];
  let tick;
  const document = {
    querySelector: element, querySelectorAll: () => [], getElementById: (id) => element(`#${id}`),
    addEventListener() {}, documentElement: { dataset: { theme: "dark" } }, body: { classList: { toggle() {} } },
  };
  const window = {
    __TAURI__: { core: { invoke(command, body) {
      return new Promise((resolve, reject) => calls.push({ command, body, resolve, reject }));
    } } },
    setInterval(fn) { tick = fn; }, addEventListener() {},
  };
  vm.runInNewContext(script, { document, window, TextEncoder });
  const click = (selector) => element(selector).listeners.click();
  const submit = (selector) => element(selector).listeners.submit({ preventDefault() {} });
  function take(command) {
    const call = calls.shift(); assert.equal(call?.command, command); return call;
  }
  function begin() {
    click("#get-started");
    element("#master-password").value = "synthetic master";
    element("#confirm-password").value = "synthetic master";
    const pending = submit("#setup-form");
    assert.equal(element("#master-password").value, "");
    assert.equal(element("#confirm-password").value, "");
    return pending;
  }
  return { element, click, submit, take, calls, begin, tick: () => tick() };
}

test("connection loss invalidates an outstanding recovery-code delivery and cancels enrollment", async () => {
  const view = renderer(); const pending = view.begin();
  view.take("begin_vault_setup").resolve({ epoch: 1, state: "recovery" }); await pending;
  const code = view.take("get_setup_recovery_code");
  const poll = view.tick(); view.take("get_vault_setup_state").reject("lost"); await poll;
  assert.equal(view.element("#result-heading").textContent, "Connection lost");
  const cancel = view.take("cancel_vault_setup"); assert.equal(cancel.body.epoch, 1); cancel.resolve();
  code.resolve({ epoch: 1, code: "SYNTHETIC CODE" }); await flush();
  assert.equal(view.element("#recovery-code").textContent, "");
  assert.equal(view.element("#setup-result").hidden, false);
  await view.tick(); assert.equal(view.calls.length, 0);
});

test("leaving during opening cancels the late native epoch without reopening the screen", async () => {
  const view = renderer(); const pending = view.begin();
  const opening = view.take("begin_vault_setup"); view.click("#setup-back");
  opening.resolve({ epoch: 4, state: "recovery" }); await pending;
  const cancel = view.take("cancel_vault_setup"); assert.equal(cancel.body.epoch, 4); cancel.resolve();
  assert.equal(view.element(".welcome").hidden, false);
  assert.equal(view.element("#recovery").hidden, true);
  assert.equal(view.element("#recovery-code").textContent, "");
});

test("navigation clears an exposed code and discards any stale status result", async () => {
  const view = renderer(); const pending = view.begin();
  view.take("begin_vault_setup").resolve({ epoch: 2, state: "recovery" }); await pending;
  view.take("get_setup_recovery_code").resolve({ epoch: 2, code: "SYNTHETIC CODE" }); await flush();
  assert.equal(view.element("#recovery-code").textContent, "SYNTHETIC CODE");
  const poll = view.tick(); const status = view.take("get_vault_setup_state");
  view.click("#recovery-back"); view.take("cancel_vault_setup").resolve();
  status.resolve({ epoch: 2, state: "recovery" }); await poll;
  assert.equal(view.element("#recovery-code").textContent, "");
  assert.equal(view.element(".welcome").hidden, false);
  assert.equal(view.calls.length, 0);
});

test("saving and unknown outcomes never display committed success", async () => {
  const view = renderer(); const pending = view.begin();
  view.take("begin_vault_setup").resolve({ epoch: 3, state: "recovery" }); await pending;
  view.take("get_setup_recovery_code").resolve({ epoch: 3, code: "SYNTHETIC CODE" }); await flush();
  view.click("#code-written");
  assert.equal(view.element("#recovery-code").textContent, "");
  view.element("#paper-code").value = "SYNTHETIC CODE";
  const save = view.submit("#recovery-form");
  assert.equal(view.element("#paper-code").value, "");
  view.take("confirm_vault_setup").resolve({ epoch: 3, state: "saving" }); await save;
  assert.equal(view.element("#setup-result").hidden, true);
  const poll = view.tick(); view.take("get_vault_setup_state").resolve({ epoch: 3, state: "unknown" }); await poll;
  assert.equal(view.element("#result-heading").textContent, "Save not confirmed");
  assert.equal(view.element("#recovery-code").textContent, "");
});

test("a lost confirmation reply checks actual status instead of claiming failure", async () => {
  const view = renderer(); const pending = view.begin();
  view.take("begin_vault_setup").resolve({ epoch: 5, state: "recovery" }); await pending;
  view.take("get_setup_recovery_code").resolve({ epoch: 5, code: "SYNTHETIC CODE" }); await flush();
  view.click("#code-written"); view.element("#paper-code").value = "SYNTHETIC CODE";
  const save = view.submit("#recovery-form"); view.take("confirm_vault_setup").reject("lost"); await save;
  assert.equal(view.element("#recovery-status").textContent, "Checking vault status…");
  assert.equal(view.element("#recovery-error").textContent, "");
  const poll = view.tick(); view.take("get_vault_setup_state").resolve({ epoch: 5, state: "created" }); await poll;
  assert.equal(view.element("#result-heading").textContent, "Vault saved");
});
