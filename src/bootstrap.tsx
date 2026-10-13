import React from "react";
import ReactDOM from "react-dom/client";
import { isMyGo } from "mygo-runtime";
import { invoke } from "./desktop";
import appIcon from "../src-tauri/icons/32x32.png";
import App from "./App";
import ExcelReader from "./ExcelReader";
import "./styles.css";

export function mountApplication() {
  const readerMode = new URLSearchParams(window.location.search).get("mode") === "reader";
  const mygoWindow = isMyGo();
  if (mygoWindow) {
    document.title = readerMode
      ? "StructSheet · Excel 阅读器"
      : "StructSheet · C 结构体转 Excel";
  }
  document.body.classList.toggle("reader-window", readerMode);
  document.body.classList.toggle("mygo-window", mygoWindow);
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      {mygoWindow && (
        <div className="mygo-titlebar">
          <img src={appIcon} alt="" />
          <span>{readerMode ? "StructSheet · Excel 阅读器" : "StructSheet · C 结构体转 Excel"}</span>
        </div>
      )}
      {readerMode
        ? <ExcelReader standalone onClose={() => void invoke("hide_excel_reader")} />
        : <App />}
    </React.StrictMode>,
  );
}
