import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { call, isMyGo } from "mygo-runtime";

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isMyGo()) return tauriInvoke<T>(command, args);

  switch (command) {
    case "list_structs":
      return call<T>("Service.ListStructs", args.source);
    case "parse_source":
      return call<T>("Service.ParseSource", args.source, args.rootName ?? null);
    case "export_excel":
      return call<T>("Service.ExportExcel", args.source, args.rootName ?? null, args.path);
    case "convert_table_to_xmacro":
      return call<T>(
        "Service.ConvertTableToXmacro",
        args.path,
        args.clipboardText,
        args.macroName,
        args.structName,
      );
    case "read_excel_document":
      return call<T>("Service.ReadExcelDocument", args.path);
    case "save_text_file":
      return call<T>("Service.SaveTextFile", args.path, args.content);
    case "choose_export_path":
      return (await call<string>("Service.ChooseExportPath", args.defaultName) || null) as T;
    case "choose_code_path":
      return (await call<string>("Service.ChooseCodePath", args.defaultName) || null) as T;
    case "choose_table_path":
      return (await call<string>("Service.ChooseTablePath") || null) as T;
    case "show_excel_reader":
      return call<T>("Service.ShowExcelReader");
    case "hide_excel_reader":
      return call<T>("Service.HideExcelReader");
    default:
      throw new Error(`不支持的桌面操作：${command}`);
  }
}
