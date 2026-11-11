const statusElement = document.querySelector("#state");
const openButton = document.querySelector("#open");
const lockButton = document.querySelector("#lock");
const labels = {
  closed: "Closed",
  opening: "Opening",
  enrollment: "Synthetic enrollment active",
  locked: "Locked",
  unavailable: "Unavailable",
};

let actionPending = false;
let pollPending = false;
let state = "unavailable";
let sequence = 0;

function render() {
  if (statusElement.textContent !== labels[state]) {
    statusElement.textContent = labels[state];
  }
  openButton.disabled = actionPending || state === "opening" || state === "enrollment";
  lockButton.disabled = actionPending || (state !== "opening" && state !== "enrollment");
}

async function request(command) {
  const action = command !== "get_session_state";
  if (actionPending || (!action && pollPending)) return;
  if (action) actionPending = true;
  else pollPending = true;
  const current = ++sequence;
  render();
  try {
    const reply = await window.__TAURI__.core.invoke(command);
    if (current === sequence) state = Object.hasOwn(labels, reply) ? reply : "unavailable";
  } catch {
    if (current === sequence) state = "unavailable";
  } finally {
    if (action) actionPending = false;
    else pollPending = false;
    render();
  }
}

openButton.addEventListener("click", () => request("open_synthetic_session"));
lockButton.addEventListener("click", () => request("lock_vault"));
request("get_session_state");
window.setInterval(() => request("get_session_state"), 250);
