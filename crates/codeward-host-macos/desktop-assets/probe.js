const statusElement = document.querySelector("#state");
const preview = document.querySelector("#preview");
const draft = document.querySelector("#draft");
const previewFixture = "Synthetic credential preview";
const draftFixture = "Synthetic unsaved edit";
const invoke = (command) => window.__TAURI__.core.invoke(command);
let armed = false;
let sealed = false;
let acknowledged = false;
let pending = false;

function clearFields() {
  sealed = true;
  preview.textContent = "";
  draft.value = "";
  preview.hidden = true;
  draft.hidden = true;
}

async function poll() {
  if (pending || acknowledged) return;
  pending = true;
  try {
    const state = await invoke("get_session_state");
    if (state === "locked" || state === "unavailable") {
      clearFields();
      statusElement.textContent = state === "locked" ? "Locked" : "Unavailable";
      if (armed && state === "locked" && preview.hidden && draft.hidden
          && preview.textContent === "" && draft.value === "") {
        await invoke("probe_cleared");
        acknowledged = true;
      }
    } else if (state === "enrollment" && !armed && !sealed
               && document.visibilityState === "visible") {
      preview.textContent = previewFixture;
      draft.value = draftFixture;
      preview.hidden = false;
      draft.hidden = false;
      if (preview.getClientRects().length && draft.getClientRects().length
          && preview.textContent === previewFixture && draft.value === draftFixture) {
        await invoke("probe_visible");
        armed = true;
        statusElement.textContent = "Armed";
      }
    }
  } catch {
    clearFields();
    statusElement.textContent = "Unavailable";
  } finally {
    pending = false;
  }
}

poll();
window.setInterval(poll, 250);
