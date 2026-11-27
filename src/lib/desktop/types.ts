import type { ResultData, StructCatalog, ParseResult } from "../mygo/bindings";
import { isMyGo } from "./host";

/**
 * 业务调用面：生产环境直连 MyGo 生成的 Sheets 绑定；浏览器与组件测试
 * 注入 mock 实现（见 desktop/index 与 test/mocks）。
 */
export interface SheetsApi {
  listStructs(source: string): Promise<StructCatalog>;
  parseSource(source: string, rootName: string): Promise<ParseResult>;
  exportExcel(source: string, rootName: string, path: string): Promise<string>;
  convertTableToXMacro(
    path: string,
    clipboardText: string,
    macroName: string,
    structName: string,
  ): Promise<ResultData>;
  saveTextFile(path: string, content: string): Promise<string>;
  readTextFile(path: string): Promise<string>;
}

export interface PickFilesOptions {
  title?: string;
  extensions?: string[];
  multiple?: boolean;
}

export interface SaveFileOptions {
  title?: string;
  defaultPath?: string;
  extensions?: string[];
}

/**
 * 桌面调用面：窗口控制、系统对话框、文件定位与版本号。
 */
export interface DesktopApi {
  kind: "mygo" | "mock";
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
  pickFiles(options?: PickFilesOptions): Promise<string[]>;
  pickFolder(options?: { title?: string }): Promise<string | null>;
  saveFile(options?: SaveFileOptions): Promise<string | null>;
  revealInFolder(path: string): Promise<void>;
  openPath(path: string): Promise<void>;
  appVersion(): Promise<string>;
}

export { isMyGo };
