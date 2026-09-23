import {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  BookOpenText,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  FileSpreadsheet,
  FolderOpen,
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

type OutlineItem = { rowNumber: number; number: string; title: string; level: number };
type CellImage = { row: number; column: number; name: string; mimeType: string; dataUrl: string };
type ExcelRow = { rowNumber: number; cells: string[] };
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
type ExcelDocument = { fileName: string; sourcePath: string; sheets: ExcelSheet[]; warnings: string[] };
type SearchMatch = { rowNumber: number; column: number };
type Props = { onClose: () => void; standalone?: boolean };

const PAGE_SIZE = 400;
const SIDEBAR_MIN = 220;
const SIDEBAR_MAX = 620;

export default function ExcelReader({ onClose, standalone = false }: Props) {
  const [documentData, setDocumentData] = useState<ExcelDocument | null>(null);
  const [activeSheetIndex, setActiveSheetIndex] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [sidebarWidth, setSidebarWidth] = useState(readSidebarWidth);
  const [outlineQuery, setOutlineQuery] = useState("");
  const [outlineDepth, setOutlineDepth] = useState(0);
  const [collapsedSections, setCollapsedSections] = useState<Set<number>>(new Set());
  const [searchQuery, setSearchQuery] = useState("");
  const [searchTerm, setSearchTerm] = useState("");
  const [searchCursor, setSearchCursor] = useState(-1);
  const [page, setPage] = useState(0);
  const [zoom, setZoom] = useState(100);
  const [wrap, setWrap] = useState(true);
  const [compact, setCompact] = useState(false);
  const [selectedRow, setSelectedRow] = useState<number | null>(null);
  const [activeCell, setActiveCell] = useState("");
  const [columnWidthsBySheet, setColumnWidthsBySheet] = useState<Record<string, number[]>>({});
  const [activeImage, setActiveImage] = useState<CellImage | null>(null);
  const [imageZoom, setImageZoom] = useState(100);
  const readerBodyRef = useRef<HTMLDivElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const pendingSearchDirection = useRef<1 | -1 | null>(null);

  const sheet = documentData?.sheets[activeSheetIndex] ?? null;
  const dataRows = useMemo(() => sheet?.rows.filter((_, index) => index !== sheet.headerRow) ?? [], [sheet]);
  const suggestedWidths = useMemo(() => (sheet ? suggestColumnWidths(sheet) : []), [sheet]);
  const columnWidths = sheet ? columnWidthsBySheet[sheet.name] ?? suggestedWidths : [];
  const tableWidth = 48 + columnWidths.reduce((total, width) => total + width, 0);
  const totalPages = Math.max(1, Math.ceil(dataRows.length / PAGE_SIZE));
  const visibleRows = useMemo(() => dataRows.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE), [dataRows, page]);
  const sectionRows = useMemo(() => new Map(sheet?.outline.map((item) => [item.rowNumber, item.level]) ?? []), [sheet]);
  const imagesByCell = useMemo(() => {
    const map = new Map<string, CellImage[]>();
    for (const image of sheet?.images ?? []) {
      const key = `${image.row}:${image.column}`;
      map.set(key, [...(map.get(key) ?? []), image]);
    }
    return map;
  }, [sheet]);
  const outlineHasChildren = useMemo(() => {
    const children = new Set<number>();
    const items = sheet?.outline ?? [];
    items.forEach((item, index) => {
      if (items[index + 1] && items[index + 1].level > item.level) children.add(item.rowNumber);
    });
    return children;
  }, [sheet]);
  const maxOutlineDepth = useMemo(() => Math.max(1, ...(sheet?.outline.map((item) => item.level) ?? [1])), [sheet]);
  const filteredOutline = useMemo(() => {
    const query = outlineQuery.trim().toLocaleLowerCase();
    const depthFiltered = (sheet?.outline ?? []).filter((item) => outlineDepth === 0 || item.level <= outlineDepth);
    if (query) {
      return depthFiltered.filter((item) => `${item.number} ${item.title}`.toLocaleLowerCase().includes(query));
    }
    const visible: OutlineItem[] = [];
    let hiddenBelowLevel: number | null = null;
    for (const item of depthFiltered) {
      if (hiddenBelowLevel !== null) {
        if (item.level > hiddenBelowLevel) continue;
        hiddenBelowLevel = null;
      }
      visible.push(item);
      if (collapsedSections.has(item.rowNumber)) hiddenBelowLevel = item.level;
    }
    return visible;
  }, [sheet, outlineDepth, outlineQuery, collapsedSections]);
  const searchResults = useMemo(() => {
    const query = searchTerm.trim().toLocaleLowerCase();
    if (!query) return [] as SearchMatch[];
    const matches: SearchMatch[] = [];
    for (const row of dataRows) {
      row.cells.forEach((cell, column) => {
        if (cell.toLocaleLowerCase().includes(query)) matches.push({ rowNumber: row.rowNumber, column });
      });
    }
    return matches;
  }, [dataRows, searchTerm]);
  const searchResultCells = useMemo(() => new Set(searchResults.map((match) => `${match.rowNumber}:${match.column}`)), [searchResults]);

  useEffect(() => {
    const timer = window.setTimeout(() => setSearchTerm(searchQuery), 160);
    return () => window.clearTimeout(timer);
  }, [searchQuery]);

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
      if (event.ctrlKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  useEffect(() => {
    setPage(0);
    setSearchCursor(-1);
    setSelectedRow(null);
    setActiveCell("");
    setCollapsedSections(new Set());
  }, [activeSheetIndex]);

  useEffect(() => {
    setSearchCursor(-1);
    setActiveCell("");
  }, [searchTerm]);

  useEffect(() => {
    if (pendingSearchDirection.current === null) return;
    const direction = pendingSearchDirection.current;
    pendingSearchDirection.current = null;
    if (searchResults.length) stepSearch(direction);
  }, [searchResults]);

  useEffect(() => {
    window.localStorage.setItem("structsheet.reader.sidebarWidth", String(sidebarWidth));
  }, [sidebarWidth]);

  async function openWorkbook() {
    setBusy(true);
    setError("");
    try {
      const path = await invoke<string | null>("choose_table_path");
      if (!path) return;
      const loaded = await invoke<ExcelDocument>("read_excel_document", { path });
      setDocumentData(loaded);
      setColumnWidthsBySheet({});
      setActiveSheetIndex(0);
      setPage(0);
      setSearchQuery("");
      setSearchTerm("");
      setOutlineQuery("");
      setOutlineDepth(0);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  function goToCell(rowNumber: number, column?: number) {
    const index = dataRows.findIndex((row) => row.rowNumber === rowNumber);
    if (index < 0) return;
    setPage(Math.floor(index / PAGE_SIZE));
    setSelectedRow(rowNumber);
    setActiveCell(column === undefined ? "" : `${rowNumber}:${column}`);
    window.setTimeout(() => {
      const selector = column === undefined ? `[data-reader-row="${rowNumber}"]` : `[data-reader-cell="${rowNumber}:${column}"]`;
      document.querySelector(selector)?.scrollIntoView({ behavior: "smooth", block: "center", inline: "center" });
    }, 30);
  }

  function stepSearch(direction: 1 | -1) {
    if (!searchResults.length) return;
    const next = searchCursor < 0
      ? direction === 1 ? 0 : searchResults.length - 1
      : direction === 1
        ? (searchCursor + 1) % searchResults.length
        : (searchCursor - 1 + searchResults.length) % searchResults.length;
    setSearchCursor(next);
    const match = searchResults[next];
    goToCell(match.rowNumber, match.column);
  }

  function handleSearchKeyDown(event: ReactKeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      const direction = event.shiftKey ? -1 : 1;
      if (searchQuery !== searchTerm) {
        pendingSearchDirection.current = direction;
        setSearchTerm(searchQuery);
      } else {
        stepSearch(direction);
      }
    }
  }

  function toggleSection(rowNumber: number) {
    setCollapsedSections((current) => {
      const next = new Set(current);
      if (next.has(rowNumber)) next.delete(rowNumber);
      else next.add(rowNumber);
      return next;
    });
  }

  function beginSidebarResize(event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = sidebarWidth;
    document.body.classList.add("is-resizing");
    const onMove = (moveEvent: PointerEvent) => setSidebarWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidth + moveEvent.clientX - startX)));
    const onUp = () => {
      document.body.classList.remove("is-resizing");
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  function updateColumnWidth(column: number, width: number) {
    if (!sheet) return;
    setColumnWidthsBySheet((current) => {
      const widths = [...(current[sheet.name] ?? suggestedWidths)];
      widths[column] = Math.min(760, Math.max(64, width));
      return { ...current, [sheet.name]: widths };
    });
  }

  function beginColumnResize(column: number, event: ReactPointerEvent<HTMLDivElement>) {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = columnWidths[column] ?? 120;
    document.body.classList.add("is-resizing");
    const onMove = (moveEvent: PointerEvent) => updateColumnWidth(column, startWidth + moveEvent.clientX - startX);
    const onUp = () => {
      document.body.classList.remove("is-resizing");
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  const workspaceStyle = {
    gridTemplateColumns: sidebarOpen ? `${sidebarWidth}px 6px minmax(0, 1fr)` : "0 0 minmax(0, 1fr)",
  } satisfies CSSProperties;

  return (
    <div className={`reader-backdrop ${standalone ? "is-standalone" : ""}`}>
      <section className="excel-reader" role="dialog" aria-modal={!standalone} aria-label="Excel 文档阅读器">
        <header className="reader-header">
          <div className="reader-title">
            <div className="tool-icon"><BookOpenText size={21} /></div>
            <div><span>EXCEL READER</span><strong title={documentData?.sourcePath}>{documentData?.fileName ?? "Excel 文档阅读器"}</strong></div>
          </div>
          <div className="reader-header-actions">
            <button className="button button-dark" disabled={busy} onClick={() => void openWorkbook()}>
              {busy ? <LoaderCircle className="spin" size={15} /> : <FolderOpen size={15} />}{documentData ? "打开其他文件" : "打开 Excel"}
            </button>
            <button className="icon-button" aria-label="关闭阅读器" onClick={onClose}><X size={18} /></button>
          </div>
        </header>

        {!documentData ? (
          <div className="reader-welcome">
            <div className="reader-welcome-icon"><FileSpreadsheet size={38} /></div>
            <h2>Excel 文档阅读器</h2>
            <p>按章节目录浏览长文档，使用全文搜索快速定位内容，并直接查看单元格中的图片。</p>
            <button className="button button-primary" disabled={busy} onClick={() => void openWorkbook()}>
              {busy ? <LoaderCircle className="spin" size={16} /> : <FolderOpen size={16} />}选择 Excel 文件
            </button>
            <small>支持 .xlsx / .xls / .xlsb / .ods，Ctrl+O 打开，Ctrl+F 搜索</small>
            {error && <div className="reader-error">{error}</div>}
          </div>
        ) : (
          <div className={`reader-workspace ${sidebarOpen ? "" : "sidebar-hidden"}`} style={workspaceStyle}>
            <aside className="reader-sidebar">
              <div className="sheet-picker">
                <label htmlFor="reader-sheet">工作表</label>
                <select id="reader-sheet" value={activeSheetIndex} onChange={(event) => setActiveSheetIndex(Number(event.target.value))}>
                  {documentData.sheets.map((item, index) => <option key={`${item.name}-${index}`} value={index}>{item.name}</option>)}
                </select>
              </div>
              <div className="outline-heading">
                <div><BookOpenText size={14} /><strong>章节目录</strong><span>{sheet?.outline.length ?? 0}</span></div>
                <div className="outline-actions">
                  <button title="全部折叠" onClick={() => setCollapsedSections(new Set(outlineHasChildren))}><ChevronsDownUp size={14} /></button>
                  <button title="全部展开" onClick={() => setCollapsedSections(new Set())}><ChevronsUpDown size={14} /></button>
                </div>
              </div>
              <div className="outline-controls">
                <label className="outline-search"><Search size={13} /><input value={outlineQuery} onChange={(event) => setOutlineQuery(event.target.value)} placeholder="筛选目录" /></label>
                <select value={outlineDepth} onChange={(event) => setOutlineDepth(Number(event.target.value))} title="显示目录层级">
                  {[3, 5, 10, 20].filter((depth) => depth < maxOutlineDepth).map((depth) => <option key={depth} value={depth}>前 {depth} 级</option>)}
                  <option value={0}>全部 {maxOutlineDepth} 级</option>
                </select>
              </div>
              <nav className="outline-list" aria-label="章节目录">
                {filteredOutline.length ? filteredOutline.map((item) => {
                  const hasChildren = outlineHasChildren.has(item.rowNumber);
                  const collapsed = collapsedSections.has(item.rowNumber);
                  return (
                    <div className="outline-node" key={`${item.rowNumber}-${item.number}`} style={{ paddingLeft: `${8 + Math.max(0, item.level - 1) * 14}px` }}>
                      {hasChildren ? (
                        <button className="outline-toggle" title={collapsed ? "展开子章节" : "折叠子章节"} onClick={() => toggleSection(item.rowNumber)}><ChevronDown className={collapsed ? "collapsed" : ""} size={13} /></button>
                      ) : <span className="outline-toggle-spacer" />}
                      <button className={`outline-link ${selectedRow === item.rowNumber ? "active" : ""}`} title={`${item.number} ${item.title}`} onClick={() => goToCell(item.rowNumber)}>
                        <span>{item.number}</span><strong>{item.title}</strong>
                      </button>
                    </div>
                  );
                }) : <div className="outline-empty">没有匹配的章节。可以使用全文搜索查找正文。</div>}
              </nav>
            </aside>

            <div className="sidebar-resizer" role="separator" aria-label="调整目录宽度" aria-orientation="vertical" onPointerDown={beginSidebarResize} />

            <div className="reader-main">
              <div className="reader-toolbar">
                <button className="reader-tool-icon" title={sidebarOpen ? "隐藏目录" : "显示目录"} onClick={() => setSidebarOpen(!sidebarOpen)}>{sidebarOpen ? <PanelLeftClose size={16} /> : <PanelLeftOpen size={16} />}</button>
                <label className="reader-search">
                  <Search size={14} />
                  <input ref={searchInputRef} value={searchQuery} onKeyDown={handleSearchKeyDown} onChange={(event) => setSearchQuery(event.target.value)} placeholder="搜索当前工作表，Enter 下一处" />
                  {searchQuery && <button title="清除搜索" onClick={() => setSearchQuery("")}><X size={13} /></button>}
                  <span>{searchTerm.trim() ? (searchResults.length ? `${searchCursor >= 0 ? searchCursor + 1 : 0} / ${searchResults.length}` : "无结果") : ""}</span>
                </label>
                <button className="reader-tool-icon" disabled={!searchResults.length} title="上一个匹配（Shift+Enter）" onClick={() => stepSearch(-1)}><ChevronLeft size={15} /></button>
                <button className="reader-tool-icon" disabled={!searchResults.length} title="下一个匹配（Enter）" onClick={() => stepSearch(1)}><ChevronRight size={15} /></button>
                <div className="toolbar-separator" />
                <button className="reader-tool-icon" title="缩小" onClick={() => setZoom(Math.max(70, zoom - 10))}><ZoomOut size={15} /></button>
                <button className="zoom-value" title="恢复 100%" onClick={() => setZoom(100)}>{zoom}%</button>
                <button className="reader-tool-icon" title="放大" onClick={() => setZoom(Math.min(170, zoom + 10))}><ZoomIn size={15} /></button>
                <button className={`reader-tool-icon ${wrap ? "active" : ""}`} title="自动换行" onClick={() => setWrap(!wrap)}><WrapText size={16} /></button>
                <button className={`reader-tool-icon ${compact ? "active" : ""}`} title="紧凑行高" onClick={() => setCompact(!compact)}><Rows3 size={16} /></button>
              </div>

              {documentData.warnings.length > 0 && <div className="reader-warning" title={documentData.warnings.join("\n")}>{documentData.warnings[0]}</div>}
              {error && <div className="reader-error reader-inline-error">{error}</div>}

              <div ref={readerBodyRef} className={`reader-table-scroll ${wrap ? "is-wrapped" : ""} ${compact ? "is-compact" : ""}`}>
                <table style={{ fontSize: `${zoom}%`, width: `${tableWidth}px` }}>
                  <colgroup><col style={{ width: "48px" }} />{columnWidths.map((width, column) => <col key={column} style={{ width: `${width}px` }} />)}</colgroup>
                  <thead><tr><th className="reader-row-number">#</th>{sheet?.headers.map((header, column) => (
                    <th key={`${header}-${column}`}><span>{header}</span><div className="column-resizer" title="拖动调整列宽，双击自动适应" onPointerDown={(event) => beginColumnResize(column, event)} onDoubleClick={() => updateColumnWidth(column, suggestedWidths[column] ?? 120)} /></th>
                  ))}</tr></thead>
                  <tbody>
                    {visibleRows.map((row) => {
                      const sectionLevel = sectionRows.get(row.rowNumber);
                      return (
                        <tr key={row.rowNumber} data-reader-row={row.rowNumber} className={`${sectionLevel ? "section-row" : ""} ${selectedRow === row.rowNumber ? "selected-row" : ""}`} style={sectionLevel ? { "--section-level": sectionLevel } as CSSProperties : undefined} onClick={() => setSelectedRow(row.rowNumber)}>
                          <td className="reader-row-number">{row.rowNumber}</td>
                          {sheet?.headers.map((_, column) => {
                            const cellKey = `${row.rowNumber}:${column}`;
                            const cellImages = imagesByCell.get(cellKey) ?? [];
                            return (
                              <td key={column} data-reader-cell={cellKey} className={`${searchResultCells.has(cellKey) ? "search-match" : ""} ${activeCell === cellKey ? "active-search-match" : ""}`} title={row.cells[column] || undefined}>
                                {row.cells[column] && <span className="cell-text"><HighlightedText text={row.cells[column]} query={searchTerm} /></span>}
                                {cellImages.map((image, imageIndex) => <img key={`${image.name}-${imageIndex}`} className="cell-image" src={image.dataUrl} alt={image.name} title="双击放大" onDoubleClick={(event) => { event.stopPropagation(); setActiveImage(image); setImageZoom(100); }} />)}
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
                <span>拖动表头边界调整列宽</span>
              </footer>
            </div>
          </div>
        )}
      </section>

      {activeImage && (
        <div className="image-lightbox" role="dialog" aria-modal="true" aria-label="图片预览" onMouseDown={(event) => { if (event.target === event.currentTarget) setActiveImage(null); }}>
          <div className="image-lightbox-toolbar">
            <strong>{activeImage.name}</strong><span>第 {activeImage.row} 行 · {columnName(activeImage.column)}</span>
            <button onClick={() => setImageZoom(Math.max(25, imageZoom - 25))}><ZoomOut size={16} /></button>
            <button onClick={() => setImageZoom(100)}>{imageZoom}%</button>
            <button onClick={() => setImageZoom(Math.min(400, imageZoom + 25))}><ZoomIn size={16} /></button>
            <button aria-label="关闭图片" onClick={() => setActiveImage(null)}><X size={18} /></button>
          </div>
          <div className="image-lightbox-canvas"><img src={activeImage.dataUrl} alt={activeImage.name} style={{ width: `${imageZoom}%` }} /></div>
        </div>
      )}
    </div>
  );
}

function HighlightedText({ text, query }: { text: string; query: string }) {
  const normalizedQuery = query.trim().toLocaleLowerCase();
  if (!normalizedQuery) return text;
  const normalizedText = text.toLocaleLowerCase();
  const parts: ReactNode[] = [];
  let position = 0;
  let match = normalizedText.indexOf(normalizedQuery);
  while (match >= 0) {
    if (match > position) parts.push(text.slice(position, match));
    parts.push(<mark key={`${match}-${parts.length}`}>{text.slice(match, match + normalizedQuery.length)}</mark>);
    position = match + normalizedQuery.length;
    match = normalizedText.indexOf(normalizedQuery, position);
  }
  if (position < text.length) parts.push(text.slice(position));
  return <>{parts}</>;
}

function suggestColumnWidths(sheet: ExcelSheet) {
  const imageColumns = new Set(sheet.images.map((image) => image.column));
  return sheet.headers.map((header, column) => {
    const normalized = header.trim().toLocaleLowerCase();
    let longest = Math.max(4, visualLength(header));
    for (const row of sheet.rows.slice(0, 300)) longest = Math.max(longest, visualLength(row.cells[column] ?? ""));
    let width = Math.min(420, Math.max(76, longest * 7 + 28));
    if (/需求|content|description|comment|说明/.test(normalized)) width = Math.max(width, 420);
    if (/标题编号|section|编号/.test(normalized)) width = Math.min(Math.max(width, 120), 190);
    if (/^id$|序号/.test(normalized)) width = Math.min(width, 90);
    if (imageColumns.has(column)) width = Math.max(width, 220);
    return width;
  });
}

function visualLength(value: string) {
  return Math.max(0, ...value.split(/\r?\n/).map((line) => [...line].reduce((length, character) => length + (character.charCodeAt(0) > 255 ? 1.8 : 1), 0)));
}

function readSidebarWidth() {
  const stored = Number(window.localStorage.getItem("structsheet.reader.sidebarWidth"));
  return Number.isFinite(stored) && stored >= SIDEBAR_MIN && stored <= SIDEBAR_MAX ? stored : 310;
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
