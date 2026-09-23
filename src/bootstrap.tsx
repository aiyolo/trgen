import React from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import ExcelReader from "./ExcelReader";
import "./styles.css";

export function mountApplication() {
  const readerMode = new URLSearchParams(window.location.search).get("mode") === "reader";
  document.body.classList.toggle("reader-window", readerMode);
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      {readerMode ? (
        <ExcelReader standalone onClose={() => void invoke("hide_excel_reader")} />
      ) : (
        <App />
      )}
    </React.StrictMode>,
  );
}
