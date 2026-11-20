const toggle = document.querySelector("#sidebar-toggle");
const homeButton = document.querySelector("#home-button");
const aboutButton = document.querySelector("#about-button");
const welcome = document.querySelector(".welcome");
const about = document.querySelector("#about");
const setup = document.querySelector("#setup");
const setupForm = document.querySelector("#setup-form");
const masterPassword = document.querySelector("#master-password");
const confirmPassword = document.querySelector("#confirm-password");
const setupError = document.querySelector("#setup-error");
const setupStatus = document.querySelector("#setup-status");

function resetSetupFeedback() {
  setupError.textContent = "";
  setupStatus.textContent = "";
  masterPassword.removeAttribute("aria-invalid");
  confirmPassword.removeAttribute("aria-invalid");
}

function clearSetup() {
  setupForm.reset();
  resetSetupFeedback();
  for (const button of document.querySelectorAll("[data-reveal]")) {
    document.getElementById(button.dataset.reveal).type = "password";
    button.textContent = "Show";
    button.setAttribute("aria-pressed", "false");
    button.setAttribute("aria-label", button.dataset.reveal === "master-password" ? "Show master password" : "Show confirmation password");
  }
}

toggle.addEventListener("click", () => {
  const collapsed = document.body.classList.toggle("sidebar-collapsed");
  toggle.setAttribute("aria-expanded", String(!collapsed));
  toggle.setAttribute("aria-label", collapsed ? "Expand sidebar" : "Collapse sidebar");
});

function showHome() {
  clearSetup();
  setup.hidden = true;
  about.hidden = true;
  welcome.hidden = false;
  homeButton.setAttribute("aria-current", "page");
  aboutButton.removeAttribute("aria-current");
}

homeButton.addEventListener("click", showHome);
aboutButton.addEventListener("click", () => {
  clearSetup();
  setup.hidden = true;
  welcome.hidden = true;
  about.hidden = false;
  homeButton.removeAttribute("aria-current");
  aboutButton.setAttribute("aria-current", "page");
  document.querySelector("#about-heading").focus();
});
document.querySelector("#about-close").addEventListener("click", () => {
  showHome();
  document.querySelector("#get-started").focus();
});
document.querySelector("#get-started").addEventListener("click", () => {
  clearSetup();
  welcome.hidden = true;
  about.hidden = true;
  setup.hidden = false;
  masterPassword.focus();
});

document.querySelector("#setup-back").addEventListener("click", () => {
  showHome();
  document.querySelector("#get-started").focus();
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
setupForm.addEventListener("submit", (event) => {
  event.preventDefault();
  resetSetupFeedback();
  let invalid;
  if (!masterPassword.value) {
    setupError.textContent = "Enter a master password.";
    invalid = masterPassword;
  } else if (new TextEncoder().encode(masterPassword.value).length > 4096) {
    setupError.textContent = "Use a password of 4,096 UTF-8 bytes or fewer.";
    invalid = masterPassword;
  } else if (masterPassword.value !== confirmPassword.value) {
    setupError.textContent = "Your passwords do not match.";
    invalid = confirmPassword;
  }
  if (invalid) {
    invalid.setAttribute("aria-invalid", "true");
    invalid.focus();
    return;
  }
  clearSetup();
  setupStatus.textContent = "Recovery-code setup is coming next. No vault has been created.";
});
window.addEventListener("pagehide", clearSetup);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden") clearSetup();
});

const themeToggle = document.querySelector("#theme-toggle");
themeToggle.addEventListener("click", () => {
  const dark = document.documentElement.dataset.theme !== "dark";
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  const label = dark ? "Switch to light mode" : "Switch to dark mode";
  themeToggle.setAttribute("aria-label", label);
  themeToggle.setAttribute("title", label);
  themeToggle.setAttribute("aria-pressed", String(dark));
  document.querySelector(".theme-label").textContent = dark ? "Light mode" : "Dark mode";
});
