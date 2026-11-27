import { useEffect, useState } from "react";
import {
  ClipboardPaste,
  Copy,
  LoaderCircle,
  Play,
  Save,
  TableProperties,
  TriangleAlert,
  Upload,
  X,
} from "lucide-react";
import { desktop, sheets } from "../lib/desktop";
import { errorMessage } from "../lib/errorMessage";
import type { ResultData } from "../lib/mygo/bindings";
import type { Notice } from "../App";

/**
 * 表格转 X-Macro 集成工具对话框。字段表来自文件选择（系统对话框）或
 * 直接粘贴的 Excel/WPS 单元格；转换与代码生成全部在 Go 侧完成。
 */
export function XMacroDialog({
  onClose,
  onNotice,
}: {
  onClose: () => void;
  onNotice: (notice: Notice) => void;
}) {
  const [tablePath, setTablePath] = useState("");
  const [clipboardTable, setClipboardTable] = useState("");
  const [commonName, setCommonName] = useState("TABLE");
  const [xMacroResult, setXMacroResult] = useState<ResultData | null>(null);
  const [tableError, setTableError] = useState("");
  const [tableBusy, setTableBusy] = useState<"select" | "convert" | "save" | null>(null);
  const hasTableInput = tablePath.trim().length > 0 || clipboardTable.trim().length > 0;

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && tableBusy === null) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, tableBusy]);

  async function selectTable() {
    setTableBusy("select");
    setTableError("");
    try {
      const paths = await desktop.pickFiles({
        title: "选择字段表文件",
        extensions: ["xlsx", "xls", "xlsb", "ods", "csv"],
        multiple: false,
      });
      if (paths.length === 0) return;
      const path = paths[0];
      setTablePath(path);
      setClipboardTable("");
      setXMacroResult(null);
      const fileName = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "TABLE";
      const stem =
        fileName
          .replace(/[^A-Za-z0-9_]+/g, "_")
          .replace(/^_+|_+$/g, "")
          .toUpperCase() || "TABLE";
      setCommonName(stem);
    } catch (reason) {
      setTableError(errorMessage(reason));
    } finally {
      setTableBusy(null);
    }
  }

  async function convertTable() {
    if (!hasTableInput) return;
    setTableBusy("convert");
    setTableError("");
    try {
      const converted = await sheets.convertTableToXMacro(
        tablePath,
        clipboardTable,
        `${commonName || "TABLE"}_FIELDS`,
        `${commonName || "TABLE"}_TYPE`,
      );
      setXMacroResult(converted);
      setCommonName(converted.macroName.replace(/_FIELDS$/, ""));
    } catch (reason) {
      setXMacroResult(null);
      setTableError(errorMessage(reason));
    } finally {
      setTableBusy(null);
    }
  }

  async function copyGeneratedCode() {
    if (!xMacroResult) return;
    try {
      await navigator.clipboard.writeText(xMacroResult.code);
      onNotice({ tone: "success", message: "代码已复制到剪贴板。" });
    } catch {
      const temporary = document.createElement("textarea");
      temporary.value = xMacroResult.code;
      temporary.style.position = "fixed";
      temporary.style.opacity = "0";
      document.body.appendChild(temporary);
      temporary.select();
      const copied = document.execCommand("copy");
      temporary.remove();
      onNotice({
        tone: copied ? "success" : "error",
        message: copied ? "代码已复制到剪贴板。" : "复制失败，请在预览框中手动复制。",
      });
    }
  }

  async function saveGeneratedCode() {
    if (!xMacroResult) return;
    setTableBusy("save");
    try {
      const path = await desktop.saveFile({
        title: "保存 C 头文件",
        defaultPath: `${xMacroResult.structName}.h`,
        extensions: ["h"],
      });
      if (!path) return;
      const savedPath = await sheets.saveTextFile(path, xMacroResult.code);
      onNotice({ tone: "success", message: `已保存：${savedPath}` });
    } catch (reason) {
      onNotice({ tone: "error", message: errorMessage(reason) });
    } finally {
      setTableBusy(null);
    }
  }

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && tableBusy === null) {
          onClose();
        }
      }}
    >
      <section className="tool-modal" role="dialog" aria-modal="true" aria-labelledby="tool-title">
        <div className="tool-modal-header">
          <div className="tool-title-wrap">
            <div className="tool-icon">
              <TableProperties size={21} />
            </div>
            <div>
              <div className="panel-kicker">INTEGRATED TOOL</div>
              <h2 id="tool-title">表格转 X-Macro</h2>
            </div>
          </div>
          <button
            className="icon-button"
            aria-label="关闭"
            disabled={tableBusy !== null}
            onClick={onClose}
          >
            <X size={17} />
          </button>
        </div>

        <div className="tool-form">
          <label className="path-field">
            <span>字段表文件</span>
            <div>
              <input
                readOnly
                value={tablePath}
                placeholder="选择 .xlsx / .xls / .xlsb / .ods / .csv"
              />
              <button
                className="button button-dark"
                disabled={tableBusy !== null}
                onClick={() => void selectTable()}
              >
                {tableBusy === "select" ? (
                  <LoaderCircle className="spin" size={15} />
                ) : (
                  <Upload size={15} />
                )}
                选择表格
              </button>
            </div>
          </label>
          <div className="input-divider">
            <span>或</span>
          </div>
          <label className="paste-field">
            <span>
              <ClipboardPaste size={13} />
              直接粘贴 Excel / WPS 单元格
            </span>
            <textarea
              value={clipboardTable}
              spellCheck={false}
              placeholder={
                "先在 Excel 中选中含表头的区域并复制，然后在这里粘贴。\n示例：Bytes    Parameter Name    Type"
              }
              onChange={(event) => {
                const value = event.target.value;
                setClipboardTable(value);
                if (value.trim()) setTablePath("");
                setXMacroResult(null);
                setTableError("");
              }}
            />
          </label>
          <div className="tool-name-grid">
            <label>
              <span>公共名称</span>
              <input
                value={commonName}
                onChange={(event) => setCommonName(event.target.value.toUpperCase())}
                placeholder="例如 EI_TO_HMGPM"
                spellCheck={false}
              />
              <small className="name-preview">
                自动生成 {commonName || "TABLE"}_FIELDS 和 {commonName || "TABLE"}_TYPE
              </small>
            </label>
          </div>
          <button
            className="button button-primary tool-run"
            disabled={!hasTableInput || tableBusy !== null}
            onClick={() => void convertTable()}
          >
            {tableBusy === "convert" ? (
              <LoaderCircle className="spin" size={16} />
            ) : (
              <Play size={16} fill="currentColor" />
            )}
            解析并生成代码
          </button>
        </div>

        {tableError && (
          <div className="tool-message tool-message-error">
            <TriangleAlert size={16} />
            <span>{tableError}</span>
          </div>
        )}

        {xMacroResult ? (
          <div className="code-result">
            <div className="code-result-meta">
              <span>
                <strong>{xMacroResult.rowCount}</strong> 个字段
              </span>
              <span>工作表：{xMacroResult.sheetName}</span>
              {xMacroResult.warnings.length > 0 && (
                <span className="code-warning" title={xMacroResult.warnings.join("\n")}>
                  <TriangleAlert size={13} /> {xMacroResult.warnings.length} 项提示
                </span>
              )}
            </div>
            <textarea
              aria-label="生成的 X-Macro 代码"
              readOnly
              spellCheck={false}
              value={xMacroResult.code}
            />
            <div className="code-actions">
              <button className="button button-quiet" onClick={() => void copyGeneratedCode()}>
                <Copy size={15} />
                复制代码
              </button>
              <button
                className="button button-dark"
                disabled={tableBusy !== null}
                onClick={() => void saveGeneratedCode()}
              >
                {tableBusy === "save" ? (
                  <LoaderCircle className="spin" size={15} />
                ) : (
                  <Save size={15} />
                )}
                保存 .h
              </button>
            </div>
          </div>
        ) : (
          <div className="tool-empty">
            <TableProperties size={30} />
            <p>
              自动识别 Bytes / Parameter Name / Type 表头，生成
              <br />
              <code>X(type, name, offset, size)</code> 形式的 C 宏。
            </p>
          </div>
        )}
      </section>
    </div>
  );
}
