import { isMyGo } from "./host";
import { createMyGoDesktop, createMyGoSheets } from "./mygo";
import type { DesktopApi, SheetsApi } from "./types";

export type {
  DesktopApi,
  SheetsApi,
  PickFilesOptions,
  SaveFileOptions,
} from "./types";

export { isMyGo, isBrowserHost } from "./host";

let desktopImpl: DesktopApi | null = null;

function resolveDesktop(): DesktopApi {
  if (desktopImpl) return desktopImpl;
  desktopImpl = isMyGo() ? createMyGoDesktop() : createBrowserDesktop();
  return desktopImpl;
}

/**
 * 桌面适配器。MyGo 窗口内使用真实绑定；浏览器（vite dev 直开或
 * jsdom 测试）返回不可用的空实现，测试通过 setDesktopForTests 注入
 * mock。
 */
export const desktop: DesktopApi = {
  get kind() {
    return resolveDesktop().kind;
  },
  minimizeWindow: () => resolveDesktop().minimizeWindow(),
  toggleMaximizeWindow: () => resolveDesktop().toggleMaximizeWindow(),
  closeWindow: () => resolveDesktop().closeWindow(),
  pickFiles: (options) => resolveDesktop().pickFiles(options),
  pickFolder: (options) => resolveDesktop().pickFolder(options),
  saveFile: (options) => resolveDesktop().saveFile(options),
  revealInFolder: (path) => resolveDesktop().revealInFolder(path),
  openPath: (path) => resolveDesktop().openPath(path),
  appVersion: () => resolveDesktop().appVersion(),
};

/** 测试专用：替换桌面实现。 */
export function setDesktopForTests(impl: DesktopApi) {
  desktopImpl = impl;
}

function createBrowserDesktop(): DesktopApi {
  return {
    kind: "mock",
    async minimizeWindow() {},
    async toggleMaximizeWindow() {},
    async closeWindow() {},
    async pickFiles() {
      return [];
    },
    async pickFolder() {
      return null;
    },
    async saveFile() {
      return null;
    },
    async revealInFolder() {},
    async openPath() {},
    async appVersion() {
      return "0.3.0";
    },
  };
}

let sheetsImpl: SheetsApi | null = null;

function resolveSheets(): SheetsApi {
  if (!sheetsImpl) {
    sheetsImpl = isMyGo() ? createMyGoSheets() : createBrowserSheets();
  }
  return sheetsImpl;
}

/**
 * 业务调用面。MyGo 内直连 Go Sheets 绑定；浏览器环境返回会抛错的
 * 占位实现（提示需在桌面应用中运行），测试用 setSheetsForTests 注入。
 */
export const sheets: SheetsApi = {
  listStructs: (source) => resolveSheets().listStructs(source),
  parseSource: (source, rootName) => resolveSheets().parseSource(source, rootName),
  exportExcel: (source, rootName, path) => resolveSheets().exportExcel(source, rootName, path),
  convertTableToXMacro: (path, clipboardText, macroName, structName) =>
    resolveSheets().convertTableToXMacro(path, clipboardText, macroName, structName),
  saveTextFile: (path, content) => resolveSheets().saveTextFile(path, content),
  readTextFile: (path) => resolveSheets().readTextFile(path),
};

/** 测试专用：替换业务实现。 */
export function setSheetsForTests(impl: SheetsApi) {
  sheetsImpl = impl;
}

function createBrowserSheets(): SheetsApi {
  const unavailable = () => {
    throw new Error("此操作需要在 StructSheet 桌面应用中运行。");
  };
  return {
    listStructs: unavailable,
    parseSource: unavailable,
    exportExcel: unavailable,
    convertTableToXMacro: unavailable,
    saveTextFile: unavailable,
    readTextFile: unavailable,
  };
}
