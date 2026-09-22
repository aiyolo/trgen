import { ChangeEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Braces,
  Check,
  ChevronDown,
  ClipboardPaste,
  Copy,
  Download,
  FileCode2,
  FileSpreadsheet,
  LoaderCircle,
  Play,
  Puzzle,
  RotateCcw,
  Save,
  TableProperties,
  TriangleAlert,
  Upload,
  X,
} from "lucide-react";

type FieldRow = {
  status: string;
  parameterName: string;
  systemParameterName: string;
  dataType: string;
  dataSize: number;
  inIoBuffer: string;
  ioBufferOffset: number;
  comments: string;
};

type ParseResult = {
  rootName: string;
  availableStructs: string[];
  totalSize: number;
  rows: FieldRow[];
  warnings: string[];
};

type StructCatalog = {
  availableStructs: string[];
  defaultRoot: string;
};

type Notice = {
  tone: "success" | "error";
  message: string;
};

type XMacroResult = {
  sourcePath: string;
  sheetName: string;
  macroName: string;
  structName: string;
  rowCount: number;
  code: string;
  warnings: string[];
};

const SAMPLE_SOURCE = `typedef struct {
    uint16_t channel;       // 通道编号
    float voltage;          // 电压值
    float temperature;      // 温度
} SensorData;

typedef struct {
    uint32_t device_id;     // 设备编号
    uint8_t status;         // 设备状态
    SensorData sensors[2];  // 两路传感器
    char name[16];          // 设备名称
} DevicePacket;`;

const columns: Array<{
  key: keyof FieldRow;
  label: string;
  className?: string;
}> = [
  { key: "status", label: "Status", className: "cell-center" },
  { key: "parameterName", label: "Parameter Name" },
  { key: "systemParameterName", label: "System Parameter Name" },
  { key: "dataType", label: "Data Type", className: "cell-center" },
  { key: "dataSize", label: "Data Size", className: "cell-number" },
  { key: "inIoBuffer", label: "InIOBuffer", className: "cell-center" },
  { key: "ioBufferOffset", label: "IOBufferOffset", className: "cell-number" },
  { key: "comments", label: "Comments" },
];

function App() {
  const [source, setSource] = useState(SAMPLE_SOURCE);
  const [rootName, setRootName] = useState("");
  const [availableStructs, setAvailableStructs] = useState<string[]>([]);
  const [result, setResult] = useState<ParseResult | null>(null);
  const [busy, setBusy] = useState<"parse" | "export" | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [tableDialogOpen, setTableDialogOpen] = useState(false);
  const [tablePath, setTablePath] = useState("");
  const [clipboardTable, setClipboardTable] = useState("");
  const [commonName, setCommonName] = useState("TABLE");
  const [xMacroResult, setXMacroResult] = useState<XMacroResult | null>(null);
  const [tableError, setTableError] = useState("");
  const [tableBusy, setTableBusy] = useState<"select" | "convert" | "save" | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const hasTableInput = tablePath.trim().length > 0 || clipboardTable.trim().length > 0;

  async function parseCode(
    code = source,
    requestedRoot: string | null = rootName || null,
  ) {
    setBusy("parse");
    setError("");
    setNotice(null);
    let catalogLoaded = false;
    try {
      const catalog = await invoke<StructCatalog>("list_structs", {
        source: code,
      });
      catalogLoaded = true;
      setAvailableStructs(catalog.availableStructs);
      const selectedRoot =
        requestedRoot && catalog.availableStructs.includes(requestedRoot)
          ? requestedRoot
          : catalog.defaultRoot;
      setRootName(selectedRoot);
      const parsed = await invoke<ParseResult>("parse_source", {
        source: code,
        rootName: selectedRoot,
      });
      setResult(parsed);
      setRootName(parsed.rootName);
    } catch (reason) {
      setResult(null);
      if (!catalogLoaded) {
        setAvailableStructs([]);
        setRootName("");
      }
      setError(String(reason));
    } finally {
      setBusy(null);
    }
  }

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4500);
    return () => window.clearTimeout(timer);
  }, [notice]);

  async function handleRootChange(event: ChangeEvent<HTMLSelectElement>) {
    const nextRoot = event.target.value;
    setRootName(nextRoot);
    await parseCode(source, nextRoot);
  }

  async function exportWorkbook() {
    if (!result) return;
    setBusy("export");
    setNotice(null);
    try {
      const path = await invoke<string | null>("choose_export_path", {
        defaultName: `${result.rootName}_字段表.xlsx`,
      });
      if (!path) {
        setBusy(null);
        return;
      }
      const savedPath = await invoke<string>("export_excel", {
        source,
        rootName: result.rootName,
        path,
      });
      setNotice({ tone: "success", message: `已导出：${savedPath}` });
    } catch (reason) {
      setNotice({ tone: "error", message: String(reason) });
    } finally {
      setBusy(null);
    }
  }

  async function importHeader(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    try {
      const content = await file.text();
      setSource(content);
      setRootName("");
      setAvailableStructs([]);
      await parseCode(content, null);
    } catch {
      setError("读取文件失败，请确认文件是 UTF-8 编码的文本文件。");
    }
  }

  function resetSample() {
    setSource(SAMPLE_SOURCE);
    setRootName("");
    setAvailableStructs([]);
    void parseCode(SAMPLE_SOURCE, null);
  }

  async function selectTable() {
    setTableBusy("select");
    setTableError("");
    try {
      const path = await invoke<string | null>("choose_table_path");
      if (!path) return;
      setTablePath(path);
      setClipboardTable("");
      setXMacroResult(null);
      const fileName = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "TABLE";
      const stem = fileName
        .replace(/[^A-Za-z0-9_]+/g, "_")
        .replace(/^_+|_+$/g, "")
        .toUpperCase() || "TABLE";
      setCommonName(stem);
    } catch (reason) {
      setTableError(String(reason));
    } finally {
      setTableBusy(null);
    }
  }

  async function convertTable() {
    if (!hasTableInput) return;
    setTableBusy("convert");
    setTableError("");
    try {
      const converted = await invoke<XMacroResult>("convert_table_to_xmacro", {
        path: tablePath,
        clipboardText: clipboardTable,
        macroName: `${commonName || "TABLE"}_FIELDS`,
        structName: `${commonName || "TABLE"}_TYPE`,
      });
      setXMacroResult(converted);
      setCommonName(converted.macroName.replace(/_FIELDS$/, ""));
    } catch (reason) {
      setXMacroResult(null);
      setTableError(String(reason));
    } finally {
      setTableBusy(null);
    }
  }

  async function copyGeneratedCode() {
    if (!xMacroResult) return;
    try {
      await navigator.clipboard.writeText(xMacroResult.code);
      setNotice({ tone: "success", message: "代码已复制到剪贴板。" });
    } catch {
      const temporary = document.createElement("textarea");
      temporary.value = xMacroResult.code;
      temporary.style.position = "fixed";
      temporary.style.opacity = "0";
      document.body.appendChild(temporary);
      temporary.select();
      const copied = document.execCommand("copy");
      temporary.remove();
      setNotice({
        tone: copied ? "success" : "error",
        message: copied ? "代码已复制到剪贴板。" : "复制失败，请在预览框中手动复制。",
      });
    }
  }

  async function saveGeneratedCode() {
    if (!xMacroResult) return;
    setTableBusy("save");
    try {
      const path = await invoke<string | null>("choose_code_path", {
        defaultName: `${xMacroResult.structName}.h`,
      });
      if (!path) return;
      const savedPath = await invoke<string>("save_text_file", {
        path,
        content: xMacroResult.code,
      });
      setNotice({ tone: "success", message: `已保存：${savedPath}` });
    } catch (reason) {
      setNotice({ tone: "error", message: String(reason) });
    } finally {
      setTableBusy(null);
    }
  }

  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <Braces size={22} strokeWidth={2.2} />
          </div>
          <div>
            <div className="brand-name">StructSheet</div>
          </div>
        </div>
        <div className="header-actions">
          <button
            className="button button-tool"
            title="读取或粘贴 Excel/CSV 字段表并生成 X-Macro 代码"
            onClick={() => setTableDialogOpen(true)}
          >
            <Puzzle size={17} />
            表格转代码
          </button>
          <button
            className="button button-primary"
            disabled={!result || busy !== null}
            onClick={() => void exportWorkbook()}
          >
            {busy === "export" ? (
              <LoaderCircle className="spin" size={17} />
            ) : (
              <Download size={17} />
            )}
            导出 Excel
          </button>
        </div>
      </header>

      <main>
        <section className="workspace">
          <div className="panel source-panel">
            <div className="panel-header">
              <div>
                <div className="panel-kicker">01 · SOURCE</div>
                <h2>
                  <FileCode2 size={19} />
                  C 结构体定义
                </h2>
              </div>
              <div className="panel-tools">
                <input
                  ref={fileInputRef}
                  className="hidden-input"
                  type="file"
                  accept=".h,.c,.txt"
                  onChange={(event) => void importHeader(event)}
                />
                <button
                  className="icon-button"
                  title="导入 .h/.c 文件"
                  onClick={() => fileInputRef.current?.click()}
                >
                  <Upload size={16} />
                </button>
                <button
                  className="icon-button"
                  title="恢复示例"
                  onClick={resetSample}
                >
                  <RotateCcw size={16} />
                </button>
              </div>
            </div>
            <div className="editor-wrap">
              <div className="editor-gutter" aria-hidden="true">
                {Array.from(
                  { length: Math.max(16, source.split("\n").length) },
                  (_, index) => (
                    <span key={index}>{index + 1}</span>
                  ),
                )}
              </div>
              <textarea
                aria-label="C 结构体源代码"
                spellCheck={false}
                value={source}
                onChange={(event) => setSource(event.target.value)}
                placeholder="在这里粘贴 typedef struct 或 struct 定义…"
              />
            </div>
            <div className="source-footer">
              <button
                className="button button-dark"
                disabled={busy !== null || source.trim().length === 0}
                onClick={() => void parseCode()}
              >
                {busy === "parse" ? (
                  <LoaderCircle className="spin" size={16} />
                ) : (
                  <Play size={16} fill="currentColor" />
                )}
                解析结构体
              </button>
            </div>
          </div>

          <div className="panel preview-panel">
            <div className="panel-header preview-header">
              <div>
                <div className="panel-kicker">02 · PREVIEW</div>
                <h2>
                  <FileSpreadsheet size={19} />
                  字段表预览
                </h2>
              </div>
              {availableStructs.length > 0 && (
                <div className="result-meta">
                  <label>
                    目标结构体
                    <span className="select-wrap">
                      <select
                        value={rootName}
                        onChange={(event) => void handleRootChange(event)}
                        disabled={busy !== null}
                      >
                        {availableStructs.map((name) => (
                          <option key={name} value={name}>
                            {name}
                          </option>
                        ))}
                      </select>
                      <ChevronDown size={14} />
                    </span>
                  </label>
                  {result && (
                    <>
                      <div className="metric">
                        <strong>{result.rows.length}</strong>
                        <span>字段</span>
                      </div>
                      <div className="metric">
                        <strong>{result.totalSize}</strong>
                        <span>字节</span>
                      </div>
                    </>
                  )}
                </div>
              )}
            </div>

            {error ? (
              <div className="state-card error-state">
                <div className="state-icon">
                  <TriangleAlert size={28} />
                </div>
                <h3>解析没有完成</h3>
                <p>{error}</p>
              </div>
            ) : !result ? (
              <div className="state-card">
                <div className="state-icon">
                  <Braces size={28} />
                </div>
                <h3>等待解析</h3>
                <p>粘贴头文件后点击“解析结构体”。</p>
              </div>
            ) : (
              <>
                {result.warnings.length > 0 && (
                  <div className="warning-bar" title={result.warnings.join("\n")}>
                    <TriangleAlert size={15} />
                    {result.warnings[0]}
                    {result.warnings.length > 1 && (
                      <span>另有 {result.warnings.length - 1} 项</span>
                    )}
                  </div>
                )}
                <div className="table-scroll">
                  <table>
                    <thead>
                      <tr>
                        {columns.map((column) => (
                          <th key={column.key}>{column.label}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {result.rows.map((row, rowIndex) => (
                        <tr key={`${row.parameterName}-${rowIndex}`}>
                          {columns.map((column) => (
                            <td
                              key={column.key}
                              className={column.className}
                              title={String(row[column.key])}
                            >
                              {column.key === "status" ? (
                                <span className="status-pill">
                                  <Check size={11} />
                                  {row[column.key]}
                                </span>
                              ) : (
                                row[column.key]
                              )}
                            </td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
                <div className="preview-footer">
                  <button
                    className="text-button"
                    disabled={busy !== null}
                    onClick={() => void exportWorkbook()}
                  >
                    <Download size={14} />
                    下载完整表格
                  </button>
                </div>
              </>
            )}
          </div>
        </section>
      </main>

      {tableDialogOpen && (
        <div
          className="modal-backdrop"
          role="presentation"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && tableBusy === null) {
              setTableDialogOpen(false);
            }
          }}
        >
          <section className="tool-modal" role="dialog" aria-modal="true" aria-labelledby="tool-title">
            <div className="tool-modal-header">
              <div className="tool-title-wrap">
                <div className="tool-icon"><TableProperties size={21} /></div>
                <div>
                  <div className="panel-kicker">INTEGRATED TOOL</div>
                  <h2 id="tool-title">表格转 X-Macro</h2>
                </div>
              </div>
              <button
                className="icon-button"
                aria-label="关闭"
                disabled={tableBusy !== null}
                onClick={() => setTableDialogOpen(false)}
              >
                <X size={17} />
              </button>
            </div>

            <div className="tool-form">
              <label className="path-field">
                <span>字段表文件</span>
                <div>
                  <input readOnly value={tablePath} placeholder="选择 .xlsx / .xls / .xlsb / .ods / .csv" />
                  <button className="button button-dark" disabled={tableBusy !== null} onClick={() => void selectTable()}>
                    {tableBusy === "select" ? <LoaderCircle className="spin" size={15} /> : <Upload size={15} />}
                    选择表格
                  </button>
                </div>
              </label>
              <div className="input-divider"><span>或</span></div>
              <label className="paste-field">
                <span><ClipboardPaste size={13} />直接粘贴 Excel / WPS 单元格</span>
                <textarea
                  value={clipboardTable}
                  spellCheck={false}
                  placeholder={'先在 Excel 中选中含表头的区域并复制，然后在这里粘贴。\n示例：Bytes    Parameter Name    Type'}
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
                {tableBusy === "convert" ? <LoaderCircle className="spin" size={16} /> : <Play size={16} fill="currentColor" />}
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
                  <span><strong>{xMacroResult.rowCount}</strong> 个字段</span>
                  <span>工作表：{xMacroResult.sheetName}</span>
                  {xMacroResult.warnings.length > 0 && (
                    <span className="code-warning" title={xMacroResult.warnings.join("\n")}>
                      <TriangleAlert size={13} /> {xMacroResult.warnings.length} 项提示
                    </span>
                  )}
                </div>
                <textarea aria-label="生成的 X-Macro 代码" readOnly spellCheck={false} value={xMacroResult.code} />
                <div className="code-actions">
                  <button className="button button-quiet" onClick={() => void copyGeneratedCode()}>
                    <Copy size={15} />复制代码
                  </button>
                  <button className="button button-dark" disabled={tableBusy !== null} onClick={() => void saveGeneratedCode()}>
                    {tableBusy === "save" ? <LoaderCircle className="spin" size={15} /> : <Save size={15} />}
                    保存 .h
                  </button>
                </div>
              </div>
            ) : (
              <div className="tool-empty">
                <TableProperties size={30} />
                <p>自动识别 Bytes / Parameter Name / Type 表头，生成<br /><code>X(type, name, offset, size)</code> 形式的 C 宏。</p>
              </div>
            )}
          </section>
        </div>
      )}

      {notice && (
        <div className={`toast toast-${notice.tone}`} role="status">
          {notice.tone === "success" ? (
            <Check size={18} />
          ) : (
            <TriangleAlert size={18} />
          )}
          <span>{notice.message}</span>
        </div>
      )}
    </div>
  );
}

export default App;
