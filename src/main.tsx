import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { desktop } from "./shared/api";
import "./styles.css";
// Janela sem decoração nativa: reserva a titlebar antes do primeiro paint.
if (desktop) document.documentElement.dataset.window = "custom";
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
