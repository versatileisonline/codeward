const toggle = document.querySelector("#sidebar-toggle");
const homeButton = document.querySelector("#home-button");
const aboutButton = document.querySelector("#about-button");
const welcome = document.querySelector(".welcome");
const about = document.querySelector("#about");

toggle.addEventListener("click", () => {
  const collapsed = document.body.classList.toggle("sidebar-collapsed");
  toggle.setAttribute("aria-expanded", String(!collapsed));
  toggle.setAttribute("aria-label", collapsed ? "Expand sidebar" : "Collapse sidebar");
});

function showHome() {
  about.hidden = true;
  welcome.hidden = false;
  homeButton.setAttribute("aria-current", "page");
  aboutButton.removeAttribute("aria-current");
}

homeButton.addEventListener("click", showHome);
aboutButton.addEventListener("click", () => {
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
  document.querySelector("#setup-message").textContent = "Vault setup is coming next.";
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
