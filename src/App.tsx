import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Braces,
  Check,
  ChevronDown,
  Download,
  FileCode2,
  FileSpreadsheet,
  LoaderCircle,
  Play,
  Puzzle,
  RotateCcw,
  TriangleAlert,
  Upload,
} from "lucide-react";
import { desktop, sheets } from "./lib/desktop";
import { errorMessage } from "./lib/errorMessage";
import { exportStem } from "./lib/exportStem";
import type { FieldRow, ParseResult, ResultData, StructCatalog } from "./lib/mygo/bindings";
import { WindowControls } from "./components/WindowControls";
import { XMacroDialog } from "./components/XMacroDialog";

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

type Notice = { tone: "success" | "error"; message: string };

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

export function App() {
  const [appVersion, setAppVersion] = useState("…");
  const [source, setSource] = useState(SAMPLE_SOURCE);
  const [rootName, setRootName] = useState("");
  const [availableStructs, setAvailableStructs] = useState<string[]>([]);
  const [result, setResult] = useState<ParseResult | null>(null);
  const [busy, setBusy] = useState<"parse" | "export" | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [tableDialogOpen, setTableDialogOpen] = useState(false);
  const sourceRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    let cancelled = false;
    desktop
      .appVersion()
      .then((version) => {
        if (!cancelled) setAppVersion(version);
      })
      .catch(() => {
        if (!cancelled) setAppVersion("dev");
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const parseCode = useCallback(
    async (code = source, requestedRoot: string | null = rootName || null) => {
      setBusy("parse");
      setError("");
      setNotice(null);
      let catalogLoaded = false;
      try {
        const catalog: StructCatalog = await sheets.listStructs(code);
        catalogLoaded = true;
        setAvailableStructs(catalog.availableStructs);
        const selectedRoot =
          requestedRoot && catalog.availableStructs.includes(requestedRoot)
            ? requestedRoot
            : catalog.defaultRoot;
        setRootName(selectedRoot);
        const parsed = await sheets.parseSource(code, selectedRoot);
        setResult(parsed);
        setRootName(parsed.rootName);
      } catch (reason) {
        setResult(null);
        if (!catalogLoaded) {
          setAvailableStructs([]);
          setRootName("");
        }
        setError(errorMessage(reason));
      } finally {
        setBusy(null);
      }
    },
    [rootName, source],
  );

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 4500);
    return () => window.clearTimeout(timer);
  }, [notice]);

  async function exportWorkbook() {
    if (!result) return;
    setBusy("export");
    setNotice(null);
    try {
      const path = await desktop.saveFile({
        title: "导出 Excel 字段表",
        defaultPath: `${exportStem(result.rootName)}.xlsx`,
        extensions: ["xlsx"],
      });
      if (!path) {
        setBusy(null);
        return;
      }
      const savedPath = await sheets.exportExcel(source, result.rootName, path);
      setNotice({ tone: "success", message: `已导出：${savedPath}` });
    } catch (reason) {
      setNotice({ tone: "error", message: errorMessage(reason) });
    } finally {
      setBusy(null);
    }
  }

  async function importHeader() {
    const paths = await desktop.pickFiles({
      title: "导入 C 源文件",
      extensions: ["h", "c", "txt"],
      multiple: false,
    });
    if (paths.length === 0) return;
    try {
      const content = await sheets.readTextFile(paths[0]);
      setSource(content);
      setRootName("");
      setAvailableStructs([]);
      await parseCode(content, null);
    } catch (reason) {
      setError(`读取文件失败：${errorMessage(reason)}`);
    }
  }

  function resetSample() {
    setSource(SAMPLE_SOURCE);
    setRootName("");
    setAvailableStructs([]);
    void parseCode(SAMPLE_SOURCE, null);
  }

  async function handleRootChange(event: React.ChangeEvent<HTMLSelectElement>) {
    const nextRoot = event.target.value;
    setRootName(nextRoot);
    await parseCode(source, nextRoot);
  }

  const meta = useMemo(() => {
    if (!result) return null;
    return { fields: result.rows.length, bytes: result.totalSize };
  }, [result]);

  return (
    <div className="app-shell">
      {/* 自绘标题栏：--app-region: drag 交给 MyGo 原生拖动 */}
      <div className="titlebar" data-drag>
        <div className="titlebar-left" data-drag>
          <img src="/structsheet-icon.png" alt="" className="titlebar-icon" data-no-window-drag />
          <span className="titlebar-title">
            StructSheet {appVersion} · C 结构体转 Excel
          </span>
        </div>
        <WindowControls />
      </div>

      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <Braces size={22} strokeWidth={2.2} />
          </div>
          <div>
            <div className="brand-name">StructSheet</div>
          </div>
          <span className="version-pill">v{appVersion}</span>
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
                <button
                  className="icon-button"
                  title="导入 .h/.c 文件"
                  onClick={() => void importHeader()}
                >
                  <Upload size={16} />
                </button>
                <button className="icon-button" title="恢复示例" onClick={resetSample}>
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
                ref={sourceRef}
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
                  {meta && (
                    <>
                      <div className="metric">
                        <strong>{meta.fields}</strong>
                        <span>字段</span>
                      </div>
                      <div className="metric">
                        <strong>{meta.bytes}</strong>
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
                        <th key="index" className="index-col">
                          #
                        </th>
                        {columns.map((column) => (
                          <th key={column.key}>{column.label}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {result.rows.map((row, rowIndex) => (
                        <tr key={`${row.parameterName}-${rowIndex}`}>
                          <td
                            key="index"
                            className="index-col"
                            aria-label={`第 ${rowIndex + 1} 行`}
                          >
                            {rowIndex + 1}
                          </td>
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
        <XMacroDialog
          onClose={() => setTableDialogOpen(false)}
          onNotice={(n: Notice) => setNotice(n)}
        />
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

export type { Notice, ResultData };
