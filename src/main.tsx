import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./app/App";
import { PetSurface } from "./features/pet/PetSurface";
import "./App.css";

const route = new URLSearchParams(window.location.search).get("window") ?? "pet";
const content = route === "pet" ? <PetSurface status="idle" /> : <App />;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {content}
  </React.StrictMode>,
);
