import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  BookOpenText,
  ChevronLeft,
  ChevronRight,
  FileSpreadsheet,
  FolderOpen,
  Image as ImageIcon,
  LoaderCircle,
  PanelLeftClose,
  PanelLeftOpen,
  Rows3,
  Search,
  WrapText,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";

type OutlineItem = {
  rowNumber: number;
  number: string;
  title: string;
  level: number;
};

type CellImage = {
  row: number;
  column: number;
  name: string;
  mimeType: string;
  dataUrl: string;
};

type ExcelRow = {
  rowNumber: number;
  cells: string[];
};

type ExcelSheet = {
  name: string;
  headers: string[];
  rows: ExcelRow[];
  outline: OutlineItem[];
  images: CellImage[];
  headerRow: number;
  totalRows: number;
  totalColumns: number;
};

type ExcelDocument = {
  fileName: string;
  sourcePath: string;
  sheets: ExcelSheet[];
  warnings: string[];
};

type Props = {
  onClose: () => void;
};

const PAGE_SIZE = 400;

export default function ExcelReader({ onClose }: Props) {
  const [documentData, setDocumentData] = useState<ExcelDocument | null>(null);
  const [activeSheetIndex, setActiveSheetIndex] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [outlineQuery, setOutlineQuery] = useState("");
  const [outlineDepth, setOutlineDepth] = useState(12);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchCursor, setSearchCursor] = useState(-1);
  const [page, setPage] = useState(0);
  const [zoom, setZoom] = useState(100);
  const [wrap, setWrap] = useState(true);
  const [compact, setCompact] = useState(false);
  const [selectedRow, setSelectedRow] = useState<number | null>(null);
  const [activeImage, setActiveImage] = useState<CellImage | null>(null);
  const [imageZoom, setImageZoom] = useState(100);
  const readerBodyRef = useRef<HTMLDivElement>(null);

  const sheet = documentData?.sheets[activeSheetIndex] ?? null;
  const dataRows = useMemo(
    () => sheet?.rows.filter((_, index) => index !== sheet.headerRow) ?? [],
    [sheet],
  );
  const totalPages = Math.max(1, Math.ceil(dataRows.length / PAGE_SIZE));
  const visibleRows = useMemo(
    () => dataRows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE),
    [dataRows, page],
  );
  const sectionRows = useMemo(
    () => new Map(sheet?.outline.map((item) => [item.rowNumber, item.level]) ?? []),
    [sheet],
  );
  const imagesByCell = useMemo(() => {
    const map = new Map<string, CellImage[]>();
    for (const image of sheet?.images ?? []) {
      const key = `${image.row}:${image.column}`;
      map.set(key, [...(map.get(key) ?? []), image]);
    }
    return map;
  }, [sheet]);
  const filteredOutline = useMemo(() => {
    const query = outlineQuery.trim().toLocaleLowerCase();
    return (sheet?.outline ?? []).filter(
      (item) =>
        item.level <= outlineDepth &&
        (!query || `${item.number} ${item.title}`.toLocaleLowerCase().includes(query)),
    );
  }, [sheet, outlineDepth, outlineQuery]);
  const searchResults = useMemo(() => {
    const query = searchQuery.trim().toLocaleLowerCase();
    if (!query) return [] as number[];
    return dataRows
      .filter((row) => row.cells.some((cell) => cell.toLocaleLowerCase().includes(query)))
      .map((row) => row.rowNumber);
  }, [dataRows, searchQuery]);
  const searchResultRows = useMemo(() => new Set(searchResults), [searchResults]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        if (activeImage) setActiveImage(null);
        else onClose();
      }
      if (event.ctrlKey && event.key.toLowerCase() === "o") {
        event.preventDefault();
        void openWorkbook();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  useEffect(() => {
    setPage(0);
    setSearchCursor(-1);
    setSelectedRow(null);
  }, [activeSheetIndex, searchQuery]);

  async function openWorkbook() {
    setBusy(true);
    setError("");
    try {
      const path = await invoke<string | null>("choose_table_path");
      if (!path) return;
      const loaded = await invoke<ExcelDocument>("read_excel_document", { path });
      setDocumentData(loaded);
      setActiveSheetIndex(0);
      setPage(0);
      setSearchQuery("");
      setOutlineQuery("");
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  function goToRow(rowNumber: number) {
    const index = dataRows.findIndex((row) => row.rowNumber === rowNumber);
    if (index < 0) return;
    const nextPage = Math.floor(index / PAGE_SIZE);
    setPage(nextPage);
    setSelectedRow(rowNumber);
    window.setTimeout(() => {
      document
        .querySelector(`[data-reader-row="${rowNumber}"]`)
        ?.scrollIntoView({ behavior: "smooth", block: "center" });
    }, 30);
  }

  function stepSearch(direction: 1 | -1) {
    if (!searchResults.length) return;
    const next =
      direction === 1
        ? (searchCursor + 1 + searchResults.length) % searchResults.length
        : (searchCursor - 1 + searchResults.length) % searchResults.length;
    setSearchCursor(next);
    goToRow(searchResults[next]);
  }

  function showImage(image: CellImage) {
    setActiveImage(image);
    setImageZoom(100);
  }

  return (
    <div className="reader-backdrop">
      <section className="excel-reader" role="dialog" aria-modal="true" aria-label="Excel 文档阅读器">
        <header className="reader-header">
          <div className="reader-title">
            <div className="tool-icon"><BookOpenText size={21} /></div>
            <div>
              <span>EXCEL READER</span>
              <strong title={documentData?.sourcePath}>{documentData?.fileName ?? "Excel 文档阅读器"}</strong>
            </div>
          </div>
          <div className="reader-header-actions">
            <button className="button button-dark" disabled={busy} onClick={() => void openWorkbook()}>
              {busy ? <LoaderCircle className="spin" size={15} /> : <FolderOpen size={15} />}
              {documentData ? "打开其他文件" : "打开 Excel"}
            </button>
            <button className="icon-button" aria-label="关闭阅读器" onClick={onClose}><X size={18} /></button>
          </div>
        </header>

        {!documentData ? (
          <div className="reader-welcome">
            <div className="reader-welcome-icon"><FileSpreadsheet size={38} /></div>
            <h2>更适合阅读的 Excel 浏览方式</h2>
            <p>自动识别章节编号和标题，生成可跳转目录；支持跨行搜索、缩放、紧凑视图和图片双击放大。</p>
            <button className="button button-primary" disabled={busy} onClick={() => void openWorkbook()}>
              {busy ? <LoaderCircle className="spin" size={16} /> : <FolderOpen size={16} />}
              选择 Excel 文件
            </button>
            <small>支持 .xlsx / .xls / .xlsb / .ods，Ctrl+O 可快速打开</small>
            {error && <div className="reader-error">{error}</div>}
          </div>
        ) : (
          <div className={`reader-workspace ${sidebarOpen ? "" : "sidebar-hidden"}`}>
            <aside className="reader-sidebar">
              <div className="sheet-picker">
                <label htmlFor="reader-sheet">工作表</label>
                <select
                  id="reader-sheet"
                  value={activeSheetIndex}
                  onChange={(event) => setActiveSheetIndex(Number(event.target.value))}
                >
                  {documentData.sheets.map((item, index) => (
                    <option key={`${item.name}-${index}`} value={index}>{item.name}</option>
                  ))}
                </select>
              </div>
              <div className="outline-heading">
                <div><BookOpenText size={14} /><strong>章节目录</strong><span>{sheet?.outline.length ?? 0}</span></div>
                <select value={outlineDepth} onChange={(event) => setOutlineDepth(Number(event.target.value))} title="显示目录层级">
                  {[3, 5, 8, 12].map((depth) => <option key={depth} value={depth}>{depth === 12 ? "全部层级" : `${depth} 级`}</option>)}
                </select>
              </div>
              <label className="outline-search">
                <Search size={13} />
                <input value={outlineQuery} onChange={(event) => setOutlineQuery(event.target.value)} placeholder="筛选目录" />
              </label>
              <nav className="outline-list" aria-label="章节目录">
                {filteredOutline.length ? filteredOutline.map((item) => (
                  <button
                    key={`${item.rowNumber}-${item.number}`}
                    className={selectedRow === item.rowNumber ? "active" : ""}
                    style={{ paddingLeft: `${12 + Math.min(item.level - 1, 7) * 11}px` }}
                    title={`${item.number} ${item.title}`}
                    onClick={() => goToRow(item.rowNumber)}
                  >
                    <span>{item.number}</span>
                    <strong>{item.title}</strong>
                  </button>
                )) : <div className="outline-empty">没有识别到章节，仍可使用全文搜索浏览。</div>}
              </nav>
            </aside>

            <div className="reader-main">
              <div className="reader-toolbar">
                <button className="reader-tool-icon" title={sidebarOpen ? "隐藏目录" : "显示目录"} onClick={() => setSidebarOpen(!sidebarOpen)}>
                  {sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}
                </button>
                <label className="reader-search">
                  <Search size={14} />
                  <input value={searchQuery} onChange={(event) => setSearchQuery(event.target.value)} placeholder="全文搜索当前工作表" />
                  <span>{searchQuery.trim() ? `${searchResults.length} 处` : ""}</span>
                </label>
                <button className="reader-tool-icon" disabled={!searchResults.length} title="上一个匹配" onClick={() => stepSearch(-1)}><ChevronLeft size={15} /></button>
                <button className="reader-tool-icon" disabled={!searchResults.length} title="下一个匹配" onClick={() => stepSearch(1)}><ChevronRight size={15} /></button>
                <div className="toolbar-separator" />
                <button className="reader-tool-icon" title="缩小" onClick={() => setZoom(Math.max(70, zoom - 10))}><ZoomOut size={15} /></button>
                <button className="zoom-value" title="恢复 100%" onClick={() => setZoom(100)}>{zoom}%</button>
                <button className="reader-tool-icon" title="放大" onClick={() => setZoom(Math.min(170, zoom + 10))}><ZoomIn size={15} /></button>
                <button className={`reader-tool-icon ${wrap ? "active" : ""}`} title="自动换行" onClick={() => setWrap(!wrap)}><WrapText size={16} /></button>
                <button className={`reader-tool-icon ${compact ? "active" : ""}`} title="紧凑行高" onClick={() => setCompact(!compact)}><Rows3 size={16} /></button>
              </div>

              {documentData.warnings.length > 0 && (
                <div className="reader-warning" title={documentData.warnings.join("\n")}>{documentData.warnings[0]}</div>
              )}
              {error && <div className="reader-error reader-inline-error">{error}</div>}

              <div
                ref={readerBodyRef}
                className={`reader-table-scroll ${wrap ? "is-wrapped" : ""} ${compact ? "is-compact" : ""}`}
              >
                <table style={{ fontSize: `${zoom}%` }}>
                  <thead>
                    <tr>
                      <th className="reader-row-number">#</th>
                      {sheet?.headers.map((header, column) => <th key={`${header}-${column}`}>{header}</th>)}
                    </tr>
                  </thead>
                  <tbody>
                    {visibleRows.map((row) => {
                      const sectionLevel = sectionRows.get(row.rowNumber);
                      const matches = searchResultRows.has(row.rowNumber);
                      return (
                        <tr
                          key={row.rowNumber}
                          data-reader-row={row.rowNumber}
                          className={`${sectionLevel ? "section-row" : ""} ${matches ? "search-match" : ""} ${selectedRow === row.rowNumber ? "selected-row" : ""}`}
                          style={sectionLevel ? { "--section-level": sectionLevel } as React.CSSProperties : undefined}
                          onClick={() => setSelectedRow(row.rowNumber)}
                        >
                          <td className="reader-row-number">{row.rowNumber}</td>
                          {sheet?.headers.map((_, column) => {
                            const cellImages = imagesByCell.get(`${row.rowNumber}:${column}`) ?? [];
                            return (
                              <td key={column} title={row.cells[column] || undefined}>
                                {row.cells[column] && <span className="cell-text">{row.cells[column]}</span>}
                                {cellImages.map((image, imageIndex) => (
                                  <button
                                    key={`${image.name}-${imageIndex}`}
                                    className="cell-image"
                                    title="双击放大图片"
                                    onDoubleClick={(event) => { event.stopPropagation(); showImage(image); }}
                                  >
                                    <img src={image.dataUrl} alt={image.name} />
                                    <span><ImageIcon size={11} />双击查看</span>
                                  </button>
                                ))}
                              </td>
                            );
                          })}
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>

              <footer className="reader-statusbar">
                <span>{sheet?.totalRows ?? 0} 行 · {sheet?.totalColumns ?? 0} 列 · {sheet?.images.length ?? 0} 张图片</span>
                <div className="reader-pagination">
                  <button disabled={page === 0} onClick={() => { setPage(page - 1); readerBodyRef.current?.scrollTo({ top: 0 }); }}><ChevronLeft size={14} />上一页</button>
                  <span>{page + 1} / {totalPages}</span>
                  <button disabled={page + 1 >= totalPages} onClick={() => { setPage(page + 1); readerBodyRef.current?.scrollTo({ top: 0 }); }}>下一页<ChevronRight size={14} /></button>
                </div>
                <span>每页最多 {PAGE_SIZE} 行</span>
              </footer>
            </div>
          </div>
        )}
      </section>

      {activeImage && (
        <div className="image-lightbox" role="dialog" aria-modal="true" aria-label="图片预览" onMouseDown={(event) => {
          if (event.target === event.currentTarget) setActiveImage(null);
        }}>
          <div className="image-lightbox-toolbar">
            <strong>{activeImage.name}</strong>
            <span>第 {activeImage.row} 行 · {columnName(activeImage.column)}</span>
            <button onClick={() => setImageZoom(Math.max(25, imageZoom - 25))}><ZoomOut size={16} /></button>
            <button onClick={() => setImageZoom(100)}>{imageZoom}%</button>
            <button onClick={() => setImageZoom(Math.min(400, imageZoom + 25))}><ZoomIn size={16} /></button>
            <button aria-label="关闭图片" onClick={() => setActiveImage(null)}><X size={18} /></button>
          </div>
          <div className="image-lightbox-canvas">
            <img src={activeImage.dataUrl} alt={activeImage.name} style={{ width: `${imageZoom}%` }} />
          </div>
        </div>
      )}
    </div>
  );
}

function columnName(index: number) {
  let value = index;
  let output = "";
  do {
    output = String.fromCharCode(65 + (value % 26)) + output;
    value = Math.floor(value / 26) - 1;
  } while (value >= 0);
  return `${output} 列`;
}
