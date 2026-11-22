import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles.css";

async function start() {
  // Vite removes this entire branch and its demo module from production builds.
  if (
    import.meta.env.DEV &&
    new URLSearchParams(location.search).get("demo") === "1" &&
    !window.__TAURI__
  ) {
    const { installDemo, showDemoBadge } = await import("./demo");
    if (installDemo()) showDemoBadge();
  }
  createRoot(document.getElementById("root")!).render(<App />);
}
void start();
