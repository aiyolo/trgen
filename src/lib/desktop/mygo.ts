import { currentWindow } from "mygo-runtime";
import { Desktop, Sheets } from "../mygo/bindings";
import type {
  DesktopApi,
  PickFilesOptions,
  SaveFileOptions,
  SheetsApi,
} from "./types";

/**
 * MyGo 生产实现：窗口控制走 mygo-runtime，系统对话框与业务调用走
 * 生成的 Go 服务绑定。
 */
export function createMyGoDesktop(): DesktopApi {
  return {
    kind: "mygo",
    minimizeWindow: () => currentWindow.minimize(),
    toggleMaximizeWindow: () => currentWindow.toggleMaximize(),
    closeWindow: () => currentWindow.close(),
    async pickFiles(options: PickFilesOptions = {}) {
      return Desktop.pickFiles(
        options.title ?? "选择文件",
        options.extensions ?? [],
        options.multiple ?? true,
      );
    },
    async pickFolder(options: { title?: string } = {}) {
      const path = await Desktop.pickFolder(options.title ?? "选择文件夹");
      return path === "" ? null : path;
    },
    async saveFile(options: SaveFileOptions = {}) {
      const path = await Desktop.saveFile(
        options.title ?? "保存文件",
        options.defaultPath ?? "",
        options.extensions ?? [],
      );
      return path === "" ? null : path;
    },
    revealInFolder: (path) => Desktop.revealInFolder(path).then(() => undefined),
    openPath: (path) => Desktop.openPath(path).then(() => undefined),
    appVersion: () => Desktop.appVersion(),
  };
}

export function createMyGoSheets(): SheetsApi {
  return {
    listStructs: (source: string) => Sheets.listStructs(source),
    parseSource: (source: string, rootName: string) => Sheets.parseSource(source, rootName),
    exportExcel: (source: string, rootName: string, path: string) =>
      Sheets.exportExcel(source, rootName, path),
    convertTableToXMacro: (
      path: string,
      clipboardText: string,
      macroName: string,
      structName: string,
    ) => Sheets.convertTableToXMacro(path, clipboardText, macroName, structName),
    saveTextFile: (path: string, content: string) => Sheets.saveTextFile(path, content),
    readTextFile: (path: string) => Sheets.readTextFile(path),
  };
}
