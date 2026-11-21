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

test("unlocking clears its password, retries a wrong password, and follows native locking", async () => {
  const view = renderer();
  const opened = view.click("#open-vault");
  view.take("open_managed_vault").resolve({ epoch: 6, state: "locked" }); await opened;
  view.element("#unlock-password").value = "wrong synthetic password";
  const attempt = view.submit("#unlock-form");
  assert.equal(view.element("#unlock-password").value, "");
  view.take("unlock_managed_vault").resolve({ epoch: 6, state: "unlocking" }); await attempt;
  assert.equal(view.element("#vault").hidden, true);
  const failure = view.tick();
  view.take("get_vault_setup_state").resolve({ epoch: 6, state: "locked", error: "wrong_password" }); await failure;
  assert.equal(view.element("#unlock-error").textContent, "Incorrect password. Try again.");
  view.element("#unlock-password").value = "synthetic password";
  const retry = view.submit("#unlock-form");
  view.take("unlock_managed_vault").resolve({ epoch: 6, state: "unlocking" }); await retry;
  const success = view.tick(); view.take("get_vault_setup_state").resolve({ epoch: 6, state: "unlocked" }); await success;
  assert.equal(view.element("#vault").hidden, false);
  const lock = view.tick(); view.take("get_vault_setup_state").resolve({ epoch: 6, state: "locked" }); await lock;
  assert.equal(view.element("#vault").hidden, true);
  assert.equal(view.element("#unlock").hidden, false);
  assert.equal(view.element("#unlock-password").value, "");
});

test("navigation while authenticating ignores a late unlocked reply", async () => {
  const view = renderer(); const opened = view.click("#open-vault");
  view.take("open_managed_vault").resolve({ epoch: 7, state: "locked" }); await opened;
  view.element("#unlock-password").value = "synthetic password";
  const attempt = view.submit("#unlock-form"); const command = view.take("unlock_managed_vault");
  view.click("#unlock-back"); view.take("cancel_vault_setup").resolve();
  command.resolve({ epoch: 7, state: "unlocked" }); await attempt;
  assert.equal(view.element("#vault").hidden, true);
  assert.equal(view.element(".welcome").hidden, false);
  assert.equal(view.element("#unlock-password").value, "");
});

test("a missing vault never starts enrollment without an explicit create action", async () => {
  const view = renderer(); const opened = view.click("#open-vault");
  view.take("open_managed_vault").resolve({ epoch: 8, state: "missing" }); await opened;
  assert.equal(view.element("#result-heading").textContent, "No vault found");
  assert.equal(view.element("#setup").hidden, true);
  assert.equal(view.calls.length, 0);
});

test("a native revocation clears typed input even when the screen was already locked", async () => {
  const view = renderer(); const opened = view.click("#open-vault");
  view.take("open_managed_vault").resolve({ epoch: 9, state: "locked", revocation: 0 }); await opened;
  view.element("#unlock-password").value = "synthetic pre-auth input";
  const poll = view.tick();
  view.take("get_vault_setup_state").resolve({ epoch: 9, state: "locked", revocation: 1 }); await poll;
  assert.equal(view.element("#unlock-password").value, "");
  assert.equal(view.element("#unlock").hidden, false);
  view.element("#unlock-password").value = "synthetic new input";
  const attempt = view.submit("#unlock-form");
  const command = view.take("unlock_managed_vault");
  assert.equal(command.body.revocation, 1);
  command.resolve({ epoch: 9, state: "unlocking", revocation: 1 }); await attempt;
});
