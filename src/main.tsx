import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { App } from "./app/App";
import "./App.css";

const queryLabel = new URLSearchParams(window.location.search).get("window");
const windowLabel = queryLabel ?? getCurrentWindow().label;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App windowLabel={windowLabel} />
  </React.StrictMode>,
);
