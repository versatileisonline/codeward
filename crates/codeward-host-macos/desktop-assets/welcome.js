const $ = (selector) => document.querySelector(selector);
const toggle = $("#sidebar-toggle");
const homeButton = $("#home-button");
const aboutButton = $("#about-button");
const welcome = $(".welcome");
const about = $("#about");
const setup = $("#setup");
const recovery = $("#recovery");
const result = $("#setup-result");
const setupForm = $("#setup-form");
const masterPassword = $("#master-password");
const confirmPassword = $("#confirm-password");
const setupError = $("#setup-error");
const setupStatus = $("#setup-status");
const paper = $("#paper-code");
let sequence = 0;
let epoch = null;
let active = false;
let transcribing = false;
let polling = false;
let fetchingCode = false;
let submitting = false;
let currentState = "idle";

function invoke(command, body = {}) {
  if (!window.__TAURI__?.core?.invoke) return Promise.reject("desktop_required");
  return window.__TAURI__.core.invoke(command, body);
}
function resetSetupFeedback() {
  setupError.textContent = "";
  setupStatus.textContent = "";
  masterPassword.removeAttribute("aria-invalid");
  confirmPassword.removeAttribute("aria-invalid");
}
function clearPasswords() {
  setupForm.reset();
  for (const button of document.querySelectorAll("[data-reveal]")) {
    document.getElementById(button.dataset.reveal).type = "password";
    button.textContent = "Show";
    button.setAttribute("aria-pressed", "false");
    button.setAttribute("aria-label", button.dataset.reveal === "master-password" ? "Show master password" : "Show confirmation password");
  }
}
function clearRecovery() {
  $("#recovery-code").textContent = "";
  paper.value = "";
  paper.removeAttribute("aria-invalid");
  $("#recovery-error").textContent = "";
  $("#recovery-status").textContent = "";
  $("#code-written").disabled = true;
}
function leaveEnrollment() {
  const previous = epoch;
  ++sequence; active = false; epoch = null; submitting = false;
  fetchingCode = false; polling = false; transcribing = false; currentState = "idle";
  clearPasswords(); clearRecovery(); resetSetupFeedback();
  $("#setup-form button[type=submit]").disabled = false;
  if (previous !== null) invoke("cancel_vault_setup", { epoch: previous }).catch(() => {});
}
function showOnly(section) {
  for (const view of [welcome, about, setup, recovery, result]) view.hidden = view !== section;
}
function showHome() {
  leaveEnrollment(); showOnly(welcome);
  homeButton.setAttribute("aria-current", "page");
  aboutButton.removeAttribute("aria-current");
}
toggle.addEventListener("click", () => {
  const collapsed = document.body.classList.toggle("sidebar-collapsed");
  toggle.setAttribute("aria-expanded", String(!collapsed));
  toggle.setAttribute("aria-label", collapsed ? "Expand sidebar" : "Collapse sidebar");
});
homeButton.addEventListener("click", showHome);
aboutButton.addEventListener("click", () => {
  leaveEnrollment(); showOnly(about);
  homeButton.removeAttribute("aria-current");
  aboutButton.setAttribute("aria-current", "page");
  $("#about-heading").focus();
});
for (const selector of ["#about-close", "#setup-back", "#recovery-back", "#result-home"]) {
  $(selector).addEventListener("click", () => { showHome(); $("#get-started").focus(); });
}
$("#get-started").addEventListener("click", () => {
  leaveEnrollment(); showOnly(setup); masterPassword.focus();
});
for (const button of document.querySelectorAll("[data-reveal]")) {
  button.addEventListener("click", () => {
    const input = document.getElementById(button.dataset.reveal);
    const show = input.type === "password";
    input.type = show ? "text" : "password";
    button.textContent = show ? "Hide" : "Show";
    button.setAttribute("aria-pressed", String(show));
    const field = input === masterPassword ? "master password" : "confirmation password";
    button.setAttribute("aria-label", `${show ? "Hide" : "Show"} ${field}`);
  });
}
setupForm.addEventListener("input", resetSetupFeedback);
setupForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (submitting || active) return;
  resetSetupFeedback();
  let invalid;
  if (!masterPassword.value) { setupError.textContent = "Enter a master password."; invalid = masterPassword; }
  else if (new TextEncoder().encode(masterPassword.value).length > 4096) {
    setupError.textContent = "Use a password of 4,096 UTF-8 bytes or fewer."; invalid = masterPassword;
  } else if (masterPassword.value !== confirmPassword.value) {
    setupError.textContent = "Your passwords do not match."; invalid = confirmPassword;
  }
  if (invalid) { invalid.setAttribute("aria-invalid", "true"); invalid.focus(); return; }
  const token = ++sequence;
  submitting = true;
  $("#setup-form button[type=submit]").disabled = true;
  setupStatus.textContent = "Preparing recovery code…";
  // IPC serializes immediately; do not retain another JavaScript password draft.
  const pending = invoke("begin_vault_setup", { password: masterPassword.value, confirmation: confirmPassword.value });
  clearPasswords();
  try {
    const status = await pending;
    if (token !== sequence) {
      invoke("cancel_vault_setup", { epoch: status.epoch }).catch(() => {});
      return;
    }
    epoch = status.epoch; active = true;
    renderStatus(status, token);
  } catch (error) {
    if (token === sequence) {
      setupError.textContent = error === "desktop_required" ? "Open the desktop app to create a vault." : "Setup couldn't start. Reopen Codeward and try again.";
      setupStatus.textContent = "";
    }
  } finally {
    if (token === sequence) { submitting = false; $("#setup-form button[type=submit]").disabled = active; }
  }
});
async function fetchCode(token) {
  if (fetchingCode || !active || currentState !== "recovery") return;
  fetchingCode = true;
  try {
    const reply = await invoke("get_setup_recovery_code", { epoch });
    if (token !== sequence || reply.epoch !== epoch || currentState !== "recovery" || transcribing) return;
    $("#recovery-code").textContent = reply.code;
    $("#code-written").disabled = false;
  } catch {
    if (token === sequence) $("#recovery-status").textContent = "Code unavailable. Cancel setup and start again.";
  } finally { if (token === sequence) fetchingCode = false; }
}
function renderStatus(status, token) {
  if (token !== sequence || status.epoch !== epoch) return;
  currentState = status.state;
  if (status.state === "opening") return;
  if (status.state === "recovery" || status.state === "saving") {
    const entering = recovery.hidden;
    showOnly(recovery);
    $("#code-display").hidden = transcribing;
    $("#recovery-form").hidden = !transcribing;
    const saving = status.state === "saving";
    $("#create-vault").disabled = saving || submitting;
    $("#view-code").disabled = saving || submitting;
    paper.disabled = saving;
    $("#recovery-status").textContent = saving ? "Saving vault…" : "";
    if (status.error === "invalid_code") {
      $("#recovery-error").textContent = "The code doesn't match. Try again.";
      paper.setAttribute("aria-invalid", "true");
    }
    if (!saving && !transcribing && !$("#recovery-code").textContent) fetchCode(token);
    if (entering) $("#recovery-heading").focus();
    return;
  }
  const descriptions = {
    created: ["Vault saved", "Your vault is saved and locked. Unlocking comes next."],
    existing: ["Vault already exists", "Your existing vault was left unchanged. Unlocking comes next."],
    locked: ["Setup locked", "Start again to continue."],
    unknown: ["Save not confirmed", "Keep your recovery code. Quit and reopen Codeward to check the vault before trying again."],
    failed: ["Setup couldn't finish", "No successful save was confirmed. Start again to retry."],
  };
  const message = descriptions[status.state];
  if (!message) return;
  const entering = result.hidden;
  clearPasswords(); clearRecovery(); showOnly(result);
  $("#result-heading").textContent = message[0];
  $("#result-description").textContent = message[1];
  if (entering) $("#result-heading").focus();
}
$("#code-written").addEventListener("click", () => {
  transcribing = true;
  $("#recovery-code").textContent = "";
  $("#code-display").hidden = true;
  $("#recovery-form").hidden = false;
  paper.focus();
});
$("#view-code").addEventListener("click", () => {
  if (currentState !== "recovery") return;
  transcribing = false; clearRecovery();
  renderStatus({ epoch, state: "recovery" }, sequence);
});
$("#recovery-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (submitting || currentState !== "recovery") return;
  if (!paper.value) { $("#recovery-error").textContent = "Enter your recovery code."; paper.focus(); return; }
  const token = ++sequence;
  polling = false; fetchingCode = false;
  submitting = true;
  $("#create-vault").disabled = true;
  $("#view-code").disabled = true;
  $("#recovery-error").textContent = "";
  const pending = invoke("confirm_vault_setup", { epoch, paper: paper.value });
  paper.value = "";
  try { renderStatus(await pending, token); }
  catch (error) {
    if (token === sequence) {
      if (error === "invalid_code") $("#recovery-error").textContent = "The code doesn't match. Try again.";
      else $("#recovery-status").textContent = "Checking vault status…";
    }
  } finally {
    if (token === sequence) {
      submitting = false;
      $("#create-vault").disabled = currentState !== "recovery";
      $("#view-code").disabled = currentState !== "recovery";
    }
  }
});
window.setInterval(async () => {
  if (!active || polling || submitting) return;
  const token = sequence;
  polling = true;
  try { renderStatus(await invoke("get_vault_setup_state"), token); }
  catch {
    if (token === sequence) {
      leaveEnrollment();
      $("#result-heading").textContent = "Connection lost";
      $("#result-description").textContent = "Keep your recovery code. Reopen Codeward to check the vault.";
      showOnly(result); active = false;
    }
  } finally { if (token === sequence) polling = false; }
}, 250);
window.addEventListener("pagehide", leaveEnrollment);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden") {
    const hadEnrollment = active || submitting;
    leaveEnrollment();
    if (hadEnrollment) showOnly(welcome);
  }
});
const themeToggle = $("#theme-toggle");
themeToggle.addEventListener("click", () => {
  const dark = document.documentElement.dataset.theme !== "dark";
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  const label = dark ? "Switch to light mode" : "Switch to dark mode";
  themeToggle.setAttribute("aria-label", label);
  themeToggle.setAttribute("title", label);
  themeToggle.setAttribute("aria-pressed", String(dark));
  $(".theme-label").textContent = dark ? "Light mode" : "Dark mode";
});
