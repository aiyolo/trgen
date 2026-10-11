use crate::{excel, excel_reader, parser, table_xmacro};
use base64::Engine as _;
use gpui::prelude::*;
use gpui::{
    div, img, point, px, rgb, size, App, Application, Bounds, ClickEvent, ClipboardItem, Context,
    ElementId, Entity, FocusHandle, HighlightStyle, Image, ImageFormat, IntoElement, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement, StyledImage, StyledText, Subscription,
    Window, WindowBounds, WindowControlArea, WindowOptions,
};
use gpui_component::{
    input::{Input, InputState},
    scroll::ScrollableElement,
    select::{Select, SelectEvent, SelectState},
    Root, TitleBar,
};
use regex::Regex;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, OnceLock},
};

const SAMPLE: &str = "typedef struct {\n    uint16_t channel;       // 通道编号\n    float voltage;          // 电压值\n    float temperature;      // 温度\n} SensorData;\n\ntypedef struct {\n    uint32_t device_id;     // 设备编号\n    uint8_t status;         // 设备状态\n    SensorData sensors[2];  // 两路传感器\n    char name[16];          // 设备名称\n} DevicePacket;";
const PAGE_ROWS: usize = 400;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Struct,
    Reader,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Outline,
    Search,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReaderTheme {
    Paper,
    Ocean,
    Mint,
    Night,
}

#[derive(Clone, Copy)]
struct ReaderPalette {
    canvas: u32,
    cell: u32,
    text: u32,
    header: u32,
    grid: u32,
    section: u32,
    selected: u32,
    search: u32,
    search_active: u32,
    row_number: u32,
}

impl ReaderTheme {
    fn next(self) -> Self {
        match self {
            Self::Paper => Self::Ocean,
            Self::Ocean => Self::Mint,
            Self::Mint => Self::Night,
            Self::Night => Self::Paper,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Paper => "护眼米白",
            Self::Ocean => "清爽蓝",
            Self::Mint => "柔和绿",
            Self::Night => "夜间深色",
        }
    }
    fn palette(self) -> ReaderPalette {
        match self {
            Self::Paper => ReaderPalette {
                canvas: 0xf5f4ef,
                cell: 0xffffff,
                text: 0x34332d,
                header: 0xe3e0d7,
                grid: 0xd7d4ca,
                section: 0xf6f3dd,
                selected: 0xfffdf0,
                search: 0xfffce1,
                search_active: 0xfff3ad,
                row_number: 0x969187,
            },
            Self::Ocean => ReaderPalette {
                canvas: 0xedf4f8,
                cell: 0xfbfdff,
                text: 0x263944,
                header: 0xdceaf1,
                grid: 0xcbdce4,
                section: 0xe2f0f6,
                selected: 0xf0f9fd,
                search: 0xfff8d8,
                search_active: 0xffe99a,
                row_number: 0x718792,
            },
            Self::Mint => ReaderPalette {
                canvas: 0xeef5f0,
                cell: 0xfcfefc,
                text: 0x2a3b32,
                header: 0xdceae0,
                grid: 0xcdded2,
                section: 0xe3f1e6,
                selected: 0xf0faf3,
                search: 0xfff8d8,
                search_active: 0xffe99a,
                row_number: 0x718277,
            },
            Self::Night => ReaderPalette {
                canvas: 0x171b20,
                cell: 0x1d2329,
                text: 0xdce5eb,
                header: 0x2b353d,
                grid: 0x364149,
                section: 0x26343a,
                selected: 0x25343d,
                search: 0x3c3826,
                search_active: 0x5a4e22,
                row_number: 0x84939d,
            },
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ReaderPrefs {
    sidebar_width: f32,
    zoom: u16,
    theme: String,
    syntax_highlight: bool,
}

impl Default for ReaderPrefs {
    fn default() -> Self {
        Self {
            sidebar_width: 310.0,
            zoom: 180,
            theme: "paper".into(),
            syntax_highlight: false,
        }
    }
}

impl ReaderPrefs {
    fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA")
            .map(|dir| PathBuf::from(dir).join("StructSheet").join("reader.json"))
    }
    fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Some(path) = Self::path() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(bytes) = serde_json::to_vec_pretty(self) {
                let _ = std::fs::write(path, bytes);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct TitleDrag {
    start_x: f32,
    start_y: f32,
}

struct StructSheet {
    focus_handle: FocusHandle,
    page: Page,
    macro_open: bool,
    source: Entity<InputState>,
    macro_name: Entity<InputState>,
    clipboard_table: Entity<InputState>,
    reader_search: Entity<InputState>,
    reader_outline_filter: Entity<InputState>,
    root_select: Entity<SelectState<Vec<String>>>,
    selected_root: Option<String>,
    catalog: Vec<String>,
    parse_result: Option<parser::ParseResult>,
    macro_path: Option<PathBuf>,
    macro_result: Option<table_xmacro::ResultData>,
    macro_error: Option<String>,
    document: Option<excel_reader::ExcelDocument>,
    sheet_index: usize,
    reader_page: usize,
    selected_outline_row: Option<usize>,
    active_cell: Option<(usize, usize)>,
    sidebar_open: bool,
    sidebar_tab: SidebarTab,
    sidebar_width: f32,
    outline_depth: usize,
    collapsed_sections: HashSet<usize>,
    search_cursor: Option<usize>,
    reader_zoom: u16,
    reader_theme: ReaderTheme,
    reader_wrap: bool,
    reader_compact: bool,
    syntax_highlight: bool,
    column_picker_open: bool,
    column_widths: Vec<f32>,
    dragging_column: Option<(usize, gpui::Pixels, f32)>,
    dragging_sidebar: Option<(gpui::Pixels, f32)>,
    dragging_title: Option<TitleDrag>,
    reader_scroll: ScrollHandle,
    pending_scroll_column: Option<usize>,
    visible_columns: Vec<bool>,
    visible_columns_by_sheet: HashMap<String, Vec<bool>>,
    column_widths_by_sheet: HashMap<String, Vec<f32>>,
    reader_images: HashMap<(usize, usize, usize), Arc<Image>>,
    reader_image_names: HashMap<(usize, usize, usize), String>,
    active_image: Option<(Arc<Image>, String, usize, usize)>,
    image_zoom: f32,
    status: String,
    error: bool,
    _subscriptions: Vec<Subscription>,
}

impl StructSheet {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);
        let prefs = ReaderPrefs::load();
        let source = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("c")
                .soft_wrap(false)
                .default_value(SAMPLE)
        });
        let macro_name = cx.new(|cx| InputState::new(window, cx).default_value("TABLE"));
        let clipboard_table = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .placeholder("粘贴 Excel/WPS 中的单元格，或选择表格文件")
        });
        let reader_search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索工作表内容"));
        let reader_outline_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("筛选章节目录"));
        let root_select = cx.new(|cx| SelectState::new(Vec::<String>::new(), None, window, cx));
        let subscriptions = vec![
            cx.subscribe(
                &root_select,
                |this, _, event: &SelectEvent<Vec<String>>, cx| {
                    if let SelectEvent::Confirm(Some(name)) = event {
                        this.parse(Some(name.clone()), cx);
                    }
                },
            ),
            cx.observe(&clipboard_table, |this, input, cx| {
                if !input.read(cx).value().trim().is_empty() {
                    this.macro_path = None;
                    this.macro_result = None;
                    this.macro_error = None;
                }
                cx.notify();
            }),
            cx.observe(&reader_search, |this, input, cx| {
                if !input.read(cx).value().trim().is_empty() {
                    this.sidebar_tab = SidebarTab::Search;
                }
                this.search_cursor = None;
                cx.notify();
            }),
            cx.observe(&reader_outline_filter, |_, _, cx| cx.notify()),
        ];
        Self {
            focus_handle,
            page: Page::Struct,
            macro_open: false,
            source,
            macro_name,
            clipboard_table,
            reader_search,
            reader_outline_filter,
            root_select,
            selected_root: None,
            catalog: Vec::new(),
            parse_result: None,
            macro_path: None,
            macro_result: None,
            macro_error: None,
            document: None,
            sheet_index: 0,
            reader_page: 0,
            selected_outline_row: None,
            active_cell: None,
            sidebar_open: true,
            sidebar_tab: SidebarTab::Outline,
            sidebar_width: prefs.sidebar_width.clamp(220.0, 620.0),
            outline_depth: 0,
            collapsed_sections: HashSet::new(),
            search_cursor: None,
            reader_zoom: prefs.zoom.clamp(90, 240),
            reader_theme: match prefs.theme.as_str() {
                "ocean" => ReaderTheme::Ocean,
                "mint" => ReaderTheme::Mint,
                "night" => ReaderTheme::Night,
                _ => ReaderTheme::Paper,
            },
            reader_wrap: true,
            reader_compact: false,
            syntax_highlight: prefs.syntax_highlight,
            column_picker_open: false,
            column_widths: Vec::new(),
            dragging_column: None,
            dragging_sidebar: None,
            dragging_title: None,
            reader_scroll: ScrollHandle::new(),
            pending_scroll_column: None,
            visible_columns: Vec::new(),
            visible_columns_by_sheet: HashMap::new(),
            column_widths_by_sheet: HashMap::new(),
            reader_images: HashMap::new(),
            reader_image_names: HashMap::new(),
            active_image: None,
            image_zoom: 1.0,
            status: "粘贴结构体定义，然后点击解析。".into(),
            error: false,
            _subscriptions: subscriptions,
        }
    }

    fn message(&mut self, message: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.status = message.into();
        self.error = error;
        cx.notify();
    }

    fn save_reader_prefs(&self) {
        ReaderPrefs {
            sidebar_width: self.sidebar_width,
            zoom: self.reader_zoom,
            theme: match self.reader_theme {
                ReaderTheme::Paper => "paper",
                ReaderTheme::Ocean => "ocean",
                ReaderTheme::Mint => "mint",
                ReaderTheme::Night => "night",
            }
            .into(),
            syntax_highlight: self.syntax_highlight,
        }
        .save();
    }

    fn parse(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        let source = self.source.read(cx).value().to_string();
        match parser::discover(&source) {
            Ok(catalog) => {
                let requested = root.filter(|name| catalog.available_structs.contains(name));
                let selected = requested.unwrap_or_else(|| catalog.default_root.clone());
                self.catalog = catalog.available_structs;
                self.selected_root = Some(selected.clone());
                match parser::parse(&source, Some(&selected)) {
                    Ok(result) => {
                        let count = result.rows.len();
                        self.parse_result = Some(result);
                        self.message(format!("已解析 {selected}：{count} 个字段。"), false, cx);
                    }
                    Err(error) => {
                        self.parse_result = None;
                        self.message(error, true, cx);
                    }
                }
            }
            Err(error) => {
                self.catalog.clear();
                self.selected_root = None;
                self.parse_result = None;
                self.message(error, true, cx);
            }
        }
    }

    fn sync_root_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let names = self.catalog.clone();
        let selected = self.selected_root.clone();
        self.root_select.update(cx, |select, cx| {
            select.set_items(names, window, cx);
            if let Some(name) = selected {
                select.set_selected_value(&name, window, cx);
            } else {
                select.set_selected_index(None, window, cx);
            }
        });
    }

    fn import_header(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("C 源文件", &["h", "c", "txt"])
                .pick_file()
                .await
            {
                let content = std::fs::read_to_string(file.path());
                let _ = this.update_in(cx, |this, window, cx| match content {
                    Ok(source) => {
                        this.source
                            .update(cx, |input, cx| input.set_value(source, window, cx));
                        this.parse(None, cx);
                        this.sync_root_select(window, cx);
                    }
                    Err(error) => this.message(format!("读取文件失败：{error}"), true, cx),
                });
            }
        })
        .detach();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self.parse_result.as_ref() else {
            return;
        };
        let default_name = export_stem(&result.root_name);
        let result = result.clone();
        cx.spawn(async move |this, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(format!("{default_name}.xlsx"))
                .add_filter("Excel 工作簿", &["xlsx"])
                .save_file()
                .await
            {
                let path = file.path();
                let output = excel::write_workbook(path, &result);
                let _ = this.update(cx, |this, cx| match output {
                    Ok(()) => this.message(format!("已导出：{}", path.display()), false, cx),
                    Err(error) => this.message(error, true, cx),
                });
            }
        })
        .detach();
    }

    fn choose_macro_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("表格", &["xlsx", "xls", "xlsb", "ods", "csv"])
                .pick_file()
                .await
            {
                let path = file.path().to_path_buf();
                let _ =
                    this.update_in(cx, |this, window, cx| this.set_macro_file(path, window, cx));
            }
        })
        .detach();
    }

    fn set_macro_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("TABLE")
            .to_string();
        self.macro_path = Some(path);
        self.macro_result = None;
        self.macro_error = None;
        self.clipboard_table
            .update(cx, |input, cx| input.set_value("", window, cx));
        let normalized = stem
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    ch.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect::<String>();
        self.macro_name
            .update(cx, |input, cx| input.set_value(normalized, window, cx));
        self.message(format!("已选择表格：{stem}"), false, cx);
    }

    fn convert_macro(&mut self, cx: &mut Context<Self>) {
        let path = self
            .macro_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let clipboard = self.clipboard_table.read(cx).value().to_string();
        let name = self.macro_name.read(cx).value().trim().to_string();
        let name = if name.is_empty() {
            "TABLE".to_string()
        } else {
            name
        };
        match table_xmacro::convert(
            path,
            clipboard,
            format!("{name}_FIELDS"),
            format!("{name}_TYPE"),
        ) {
            Ok(result) => {
                let count = result.row_count;
                self.macro_result = Some(result);
                self.macro_error = None;
                self.message(format!("已生成 {count} 个字段。"), false, cx);
            }
            Err(error) => {
                self.macro_result = None;
                self.macro_error = Some(error.clone());
                self.message(error, true, cx);
            }
        }
    }

    fn save_macro(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self.macro_result.as_ref() else {
            return;
        };
        let file_name = format!("{}.h", result.struct_name);
        let code = result.code.clone();
        cx.spawn(async move |this, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .set_file_name(file_name)
                .add_filter("C 头文件", &["h"])
                .save_file()
                .await
            {
                let path = file.path();
                let output = std::fs::write(path, code);
                let _ = this.update(cx, |this, cx| match output {
                    Ok(()) => this.message(format!("已保存：{}", path.display()), false, cx),
                    Err(error) => this.message(format!("保存失败：{error}"), true, cx),
                });
            }
        })
        .detach();
    }

    fn copy_macro(&mut self, cx: &mut Context<Self>) {
        if let Some(result) = &self.macro_result {
            cx.write_to_clipboard(ClipboardItem::new_string(result.code.clone()));
            self.message("代码已复制到剪贴板。", false, cx);
        }
    }

    fn open_workbook(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            if let Some(file) = rfd::AsyncFileDialog::new()
                .add_filter("工作簿", &["xlsx", "xls", "xlsb", "ods", "csv"])
                .pick_file()
                .await
            {
                let path = file.path().to_path_buf();
                let _ = this.update_in(cx, |this, window, cx| this.load_workbook(path, window, cx));
            }
        })
        .detach();
    }

    fn load_workbook(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match excel_reader::read_document(&path.to_string_lossy()) {
            Ok(document) => {
                self.visible_columns_by_sheet.clear();
                self.column_widths_by_sheet.clear();
                self.reader_images = decode_images(&document);
                self.reader_image_names = document
                    .sheets
                    .iter()
                    .enumerate()
                    .flat_map(|(sheet, data)| {
                        data.images.iter().map(move |image| {
                            ((sheet, image.row, image.column), image.name.clone())
                        })
                    })
                    .collect();
                self.visible_columns = document
                    .sheets
                    .first()
                    .map(|sheet| vec![true; sheet.total_columns])
                    .unwrap_or_default();
                self.column_widths = document
                    .sheets
                    .first()
                    .map(suggest_column_widths)
                    .unwrap_or_default();
                let name = document.file_name.clone();
                self.document = Some(document);
                self.sheet_index = 0;
                self.reader_page = 0;
                self.selected_outline_row = None;
                self.active_cell = None;
                self.search_cursor = None;
                self.collapsed_sections.clear();
                self.outline_depth = 0;
                self.sidebar_tab = SidebarTab::Outline;
                self.reader_search
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.reader_outline_filter
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.active_image = None;
                self.message(format!("已打开：{name}"), false, cx);
            }
            Err(error) => self.message(error, true, cx),
        }
    }

    fn select_sheet(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(current) = self
            .document
            .as_ref()
            .and_then(|d| d.sheets.get(self.sheet_index))
        {
            self.visible_columns_by_sheet
                .insert(current.name.clone(), self.visible_columns.clone());
            self.column_widths_by_sheet
                .insert(current.name.clone(), self.column_widths.clone());
        }
        self.sheet_index = index;
        self.reader_page = 0;
        self.selected_outline_row = None;
        self.active_cell = None;
        self.search_cursor = None;
        self.collapsed_sections.clear();
        self.outline_depth = 0;
        self.sidebar_tab = SidebarTab::Outline;
        self.active_image = None;
        self.visible_columns = self
            .document
            .as_ref()
            .and_then(|d| d.sheets.get(index))
            .map(|s| {
                self.visible_columns_by_sheet
                    .get(&s.name)
                    .cloned()
                    .unwrap_or_else(|| vec![true; s.total_columns])
            })
            .unwrap_or_default();
        self.column_widths = self
            .document
            .as_ref()
            .and_then(|d| d.sheets.get(index))
            .map(|s| {
                self.column_widths_by_sheet
                    .get(&s.name)
                    .cloned()
                    .unwrap_or_else(|| suggest_column_widths(s))
            })
            .unwrap_or_default();
        cx.notify();
    }

    fn go_to_cell(&mut self, row_number: usize, column: Option<usize>, cx: &mut Context<Self>) {
        let Some(sheet) = self
            .document
            .as_ref()
            .and_then(|d| d.sheets.get(self.sheet_index))
        else {
            return;
        };
        let Some(index) = sheet
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != sheet.header_row)
            .position(|(_, row)| row.row_number == row_number)
        else {
            return;
        };
        self.reader_page = index / PAGE_ROWS;
        self.selected_outline_row = Some(row_number);
        self.active_cell = column.map(|column| (row_number, column));
        if let Some(column) = column {
            if let Some(visible) = self.visible_columns.get_mut(column) {
                *visible = true;
            }
            self.pending_scroll_column = Some(column);
        }
        self.reader_scroll.scroll_to_item(index % PAGE_ROWS + 1);
        cx.notify();
    }

    fn reveal_column(&mut self, column: usize) {
        let left: f32 = self
            .visible_columns
            .iter()
            .enumerate()
            .take(column)
            .filter(|(_, visible)| **visible)
            .map(|(index, _)| self.column_widths.get(index).copied().unwrap_or(180.0))
            .sum::<f32>()
            + 48.0;
        let width = self.column_widths.get(column).copied().unwrap_or(180.0);
        let viewport = f32::from(self.reader_scroll.bounds().size.width);
        let current = self.reader_scroll.offset();
        let current_left = -f32::from(current.x);
        let target_left = if left < current_left + 48.0 {
            left - 48.0
        } else if left + width > current_left + viewport {
            left + width - viewport + 8.0
        } else {
            current_left
        };
        self.reader_scroll
            .set_offset(point(px(-target_left.max(0.0)), current.y));
    }

    fn search_matches(&self, cx: &mut Context<Self>) -> Vec<(usize, usize)> {
        let query = self.reader_search.read(cx).value().trim().to_lowercase();
        let Some(sheet) = self
            .document
            .as_ref()
            .and_then(|d| d.sheets.get(self.sheet_index))
        else {
            return Vec::new();
        };
        if query.is_empty() {
            return Vec::new();
        }
        sheet
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != sheet.header_row)
            .flat_map(|(_, row)| {
                row.cells
                    .iter()
                    .enumerate()
                    .filter(|(_, cell)| cell.to_lowercase().contains(&query))
                    .map(move |(column, _)| (row.row_number, column))
            })
            .collect()
    }

    fn step_search(&mut self, direction: i32, cx: &mut Context<Self>) {
        let matches = self.search_matches(cx);
        if matches.is_empty() {
            return;
        }
        let next = match self.search_cursor {
            Some(index) if direction >= 0 => (index + 1) % matches.len(),
            Some(0) => matches.len() - 1,
            Some(index) => index - 1,
            None if direction < 0 => matches.len() - 1,
            None => 0,
        };
        self.search_cursor = Some(next);
        let (row, column) = matches[next];
        self.go_to_cell(row, Some(column), cx);
    }

    fn toggle_column(&mut self, column: usize, cx: &mut Context<Self>) {
        let visible_count = self
            .visible_columns
            .iter()
            .filter(|visible| **visible)
            .count();
        if let Some(visible) = self.visible_columns.get_mut(column) {
            if *visible && visible_count == 1 {
                return;
            }
            *visible = !*visible;
        }
        cx.notify();
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(48.0))
            .py(px(15.0))
            .bg(rgb(0xfbfaf6))
            .border_b_1()
            .border_color(rgb(0xdcd8cc))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(40.0))
                            .h(px(40.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0xf4e322))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_size(px(22.0))
                            .child("{ }"),
                    )
                    .child(
                        div()
                            .text_size(px(18.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(rgb(0x28271f))
                            .child("StructSheet"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(button(
                        "Excel 阅读器",
                        cx.listener(|_, _, _, cx| {
                            let bounds = Bounds::centered(None, size(px(1450.0), px(800.0)), cx);
                            let _ = cx.open_window(
                                WindowOptions {
                                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                                    titlebar: Some(TitleBar::title_bar_options()),
                                    ..Default::default()
                                },
                                |window, cx| {
                                    window.set_window_title("StructSheet · Excel 阅读器");
                                    let view = cx.new(|cx| {
                                        let mut view = StructSheet::new(window, cx);
                                        view.page = Page::Reader;
                                        view
                                    });
                                    cx.new(|cx| Root::new(view, window, cx))
                                },
                            );
                        }),
                    ))
                    .child(button(
                        "表格转代码",
                        cx.listener(|this, _, _, cx| {
                            this.macro_open = true;
                            cx.notify();
                        }),
                    ))
                    .child(if self.parse_result.is_some() {
                        primary_button("导出 Excel", cx.listener(|this, _, _, cx| this.export(cx)))
                            .into_any_element()
                    } else {
                        disabled_button("导出 Excel").into_any_element()
                    }),
            )
    }

    fn render_reader_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .py_2()
            .bg(rgb(0xfbfaf6))
            .text_color(rgb(0x28271f))
            .border_b_1()
            .border_color(rgb(0xdcd8cc))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(40.0))
                            .h(px(40.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0xf4e322))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_size(px(20.0))
                            .child("▣"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .text_color(rgb(0x737169))
                                    .child("EXCEL READER"),
                            )
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(
                                self.document
                                    .as_ref()
                                    .map(|d| d.file_name.clone())
                                    .unwrap_or_else(|| "Excel 文档阅读器".into()),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(dark_button(
                        "打开 Excel",
                        cx.listener(|this, _, window, cx| this.open_workbook(window, cx)),
                    ))
                    .child(button(
                        "关闭",
                        cx.listener(|_, _, window, _| window.remove_window()),
                    )),
            )
    }

    fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self.page == Page::Reader {
            "StructSheet · Excel 阅读器"
        } else {
            "StructSheet · C 结构体转 Excel"
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(34.0))
            .flex_shrink_0()
            .bg(rgb(0xfbfaf6))
            .border_b_1()
            .border_color(rgb(0xdcd8cc))
            .child(
                div()
                    .id("title-drag")
                    .flex()
                    .items_center()
                    .gap_2()
                    .h_full()
                    .flex_1()
                    .px_3()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, window, _| {
                            this.dragging_title = title_drag_start(window, event.position);
                        }),
                    )
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(
                        div()
                            .w(px(17.0))
                            .h(px(17.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(0xf4e322))
                            .child(if self.page == Page::Reader {
                                "▣"
                            } else {
                                "{}"
                            }),
                    )
                    .child(title),
            )
            .child(title_control("−", WindowControlArea::Min))
            .child(title_control(
                if window.is_maximized() { "❐" } else { "□" },
                WindowControlArea::Max,
            ))
            .child(title_control("×", WindowControlArea::Close))
    }

    fn render_struct(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let result = self.parse_result.as_ref();
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .px(px(48.0))
            .py(px(20.0))
            .child(
                div()
                    .w(px(375.0))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .min_h_0()
                    .p_3()
                    .bg(rgb(0xfbfaf6))
                    .border_1()
                    .border_color(rgb(0xdcd8cc))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(0x737169))
                            .child("01 · SOURCE"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(panel_title("C 结构体定义"))
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(button(
                                        "导入 .h/.c",
                                        cx.listener(|this, _, window, cx| {
                                            this.import_header(window, cx)
                                        }),
                                    ))
                                    .child(button(
                                        "恢复示例",
                                        cx.listener(|this, _, window, cx| {
                                            this.source.update(cx, |input, cx| {
                                                input.set_value(SAMPLE, window, cx)
                                            });
                                            this.parse(None, cx);
                                            this.sync_root_select(window, cx);
                                        }),
                                    )),
                            ),
                    )
                    .child(
                        Input::new(&self.source)
                            .h_full()
                            .bg(rgb(0x25251f))
                            .text_color(rgb(0xf5f2e8))
                            .text_size(px(12.0))
                            .font_family("Cascadia Mono")
                            .border_0()
                            .rounded_none(),
                    )
                    .child(div().flex().justify_end().child(dark_button(
                        "解析结构体",
                        cx.listener(|this, _, window, cx| {
                            this.parse(this.selected_root.clone(), cx);
                            this.sync_root_select(window, cx);
                        }),
                    ))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .bg(rgb(0xfbfaf6))
                    .border_1()
                    .border_color(rgb(0xdcd8cc))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(0x737169))
                            .child("02 · PREVIEW"),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(panel_title("字段表预览"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .children(self.selected_root.as_ref().map(|_| {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(px(11.0))
                                                    .text_color(rgb(0x737169))
                                                    .child("目标结构体"),
                                            )
                                            .child(
                                                Select::new(&self.root_select)
                                                    .w(px(156.0))
                                                    .text_size(px(12.0)),
                                            )
                                    }))
                                    .children(result.map(|r| {
                                        div()
                                            .flex()
                                            .gap_3()
                                            .child(format!("{} 字段", r.rows.len()))
                                            .child(format!("{} 字节", r.total_size))
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_scrollbar()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xdcd8cc))
                            .child(
                                div()
                                    .w_full()
                                    .h_full()
                                    .min_w(px(if result.is_some() { 1530.0 } else { 0.0 }))
                                    .flex()
                                    .flex_col()
                                    .children(result.map(|_| {
                                        field_row(
                                            [
                                                "Status",
                                                "Parameter Name",
                                                "System Parameter Name",
                                                "Data Type",
                                                "Data Size",
                                                "InIOBuffer",
                                                "IOBufferOffset",
                                                "Comments",
                                            ],
                                            true,
                                            false,
                                        )
                                    }))
                                    .children(result.is_none().then(|| {
                                        div()
                                            .p_5()
                                            .w_full()
                                            .h_full()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .justify_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .w(px(64.0))
                                                    .h(px(64.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .bg(rgb(0xefede6))
                                                    .border_1()
                                                    .border_color(rgb(0xdcd8cc))
                                                    .text_size(px(26.0))
                                                    .text_color(rgb(0x737169))
                                                    .child("{ }"),
                                            )
                                            .child(panel_title(if self.error {
                                                "解析没有完成"
                                            } else {
                                                "等待解析"
                                            }))
                                            .child(if self.error {
                                                self.status.clone()
                                            } else {
                                                "粘贴头文件后点击“解析结构体”。".into()
                                            })
                                    }))
                                    .children(
                                        result
                                            .into_iter()
                                            .flat_map(|r| r.rows.iter())
                                            .enumerate()
                                            .map(|(i, row)| {
                                                field_row(
                                                    [
                                                        row.status.as_str(),
                                                        row.parameter_name.as_str(),
                                                        row.system_parameter_name.as_str(),
                                                        row.data_type.as_str(),
                                                        &row.data_size.to_string(),
                                                        row.in_io_buffer.as_str(),
                                                        &row.io_buffer_offset.to_string(),
                                                        row.comments.as_str(),
                                                    ],
                                                    false,
                                                    i % 2 == 0,
                                                )
                                            }),
                                    ),
                            ),
                    )
                    .children(
                        result
                            .into_iter()
                            .flat_map(|r| r.warnings.iter())
                            .take(5)
                            .map(|warning| {
                                div()
                                    .text_color(rgb(0xa76314))
                                    .text_size(px(12.0))
                                    .child(warning.clone())
                            }),
                    )
                    .children(result.map(|_| {
                        div().flex().justify_end().child(button(
                            "下载完整表格",
                            cx.listener(|this, _, _, cx| this.export(cx)),
                        ))
                    })),
            )
    }

    fn render_macro(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let has_input =
            self.macro_path.is_some() || !self.clipboard_table.read(cx).value().trim().is_empty();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .bg(rgb(0xfbfaf6))
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(rgb(0xdcd8cc))
                    .child(div().text_size(px(11.0)).child("字段表文件"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .p_2()
                                    .bg(rgb(0xffffff))
                                    .border_1()
                                    .border_color(rgb(0xdcd8cc))
                                    .text_size(px(12.0))
                                    .child(
                                        self.macro_path
                                            .as_ref()
                                            .map(|path| path.display().to_string())
                                            .unwrap_or_else(|| {
                                                "选择 .xlsx / .xls / .xlsb / .ods / .csv".into()
                                            }),
                                    ),
                            )
                            .child(dark_button(
                                "选择表格",
                                cx.listener(|this, _, window, cx| {
                                    this.choose_macro_file(window, cx)
                                }),
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(0x928f83))
                            .child("或直接粘贴 Excel / WPS 单元格"),
                    )
                    .child(Input::new(&self.clipboard_table).h(px(88.0)))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_size(px(11.0)).child("公共名称"))
                                    .child(Input::new(&self.macro_name)),
                            )
                            .child(if has_input {
                                primary_button(
                                    "解析并生成代码",
                                    cx.listener(|this, _, _, cx| this.convert_macro(cx)),
                                )
                                .into_any_element()
                            } else {
                                disabled_button("解析并生成代码").into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(rgb(0x737169))
                            .child(format!(
                                "自动生成 {}_FIELDS 和 {}_TYPE",
                                self.macro_name.read(cx).value().trim(),
                                self.macro_name.read(cx).value().trim()
                            )),
                    )
                    .children(self.macro_error.iter().map(|error| {
                        div()
                            .p_2()
                            .bg(rgb(0xffe4df))
                            .text_color(rgb(0x9b3028))
                            .child(error.clone())
                    })),
            )
            .child(if let Some(result) = self.macro_result.as_ref() {
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_size(px(12.0))
                            .child(format!("{} 个字段", result.row_count))
                            .child(format!("工作表：{}", result.sheet_name))
                            .child(format!("{} 项提示", result.warnings.len()))
                            .child(div().flex_1())
                            .child(button(
                                "复制代码",
                                cx.listener(|this, _, _, cx| this.copy_macro(cx)),
                            ))
                            .child(primary_button(
                                "保存 .h 文件",
                                cx.listener(|this, _, _, cx| this.save_macro(cx)),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_scrollbar()
                            .p_3()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xdcd8cc))
                            .font_family("Consolas")
                            .text_size(px(12.0))
                            .children(result.code.lines().map(|line| {
                                div().whitespace_nowrap().child(if line.is_empty() {
                                    " ".to_string()
                                } else {
                                    line.to_string()
                                })
                            })),
                    )
                    .children(result.warnings.iter().take(5).map(|warning| {
                        div()
                            .text_color(rgb(0xa76314))
                            .text_size(px(11.0))
                            .child(warning.clone())
                    }))
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .text_color(rgb(0x737169))
                    .child(div().text_size(px(28.0)).child("▦"))
                    .child("自动识别 Bytes / Parameter Name / Type 表头，生成")
                    .child("X(type, name, offset, size) 形式的 C 宏。")
                    .into_any_element()
            })
    }
    fn render_reader_controls(
        &self,
        sheet: &excel_reader::ExcelSheet,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let palette = self.reader_theme.palette();
        div()
            .flex()
            .items_center()
            .gap_1()
            .p_1()
            .bg(rgb(palette.canvas))
            .border_b_1()
            .border_color(rgb(palette.grid))
            .child(button(
                if self.sidebar_open { "◧" } else { "▣" },
                cx.listener(|this, _, _, cx| {
                    this.sidebar_open = !this.sidebar_open;
                    cx.notify();
                }),
            ))
            .child(button(
                format!(
                    "显示列 {}/{}",
                    self.visible_columns
                        .iter()
                        .filter(|visible| **visible)
                        .count(),
                    sheet.total_columns
                ),
                cx.listener(|this, _, _, cx| {
                    this.column_picker_open = !this.column_picker_open;
                    cx.notify();
                }),
            ))
            .child(div().flex_1())
            .child(button(
                "A−",
                cx.listener(|this, _, _, cx| {
                    this.reader_zoom = this.reader_zoom.saturating_sub(10).max(90);
                    this.save_reader_prefs();
                    cx.notify();
                }),
            ))
            .child(button(
                format!("{}%", self.reader_zoom),
                cx.listener(|this, _, _, cx| {
                    this.reader_zoom = 180;
                    this.save_reader_prefs();
                    cx.notify();
                }),
            ))
            .child(button(
                "A+",
                cx.listener(|this, _, _, cx| {
                    this.reader_zoom = (this.reader_zoom + 10).min(240);
                    this.save_reader_prefs();
                    cx.notify();
                }),
            ))
            .child(button(
                format!("主题：{}", self.reader_theme.label()),
                cx.listener(|this, _, _, cx| {
                    this.reader_theme = this.reader_theme.next();
                    this.save_reader_prefs();
                    cx.notify();
                }),
            ))
            .child(nav_button(
                "高亮",
                self.syntax_highlight,
                cx.listener(|this, _, _, cx| {
                    this.syntax_highlight = !this.syntax_highlight;
                    this.save_reader_prefs();
                    cx.notify();
                }),
            ))
            .child(nav_button(
                "换行",
                self.reader_wrap,
                cx.listener(|this, _, _, cx| {
                    this.reader_wrap = !this.reader_wrap;
                    cx.notify();
                }),
            ))
            .child(nav_button(
                "紧凑",
                self.reader_compact,
                cx.listener(|this, _, _, cx| {
                    this.reader_compact = !this.reader_compact;
                    cx.notify();
                }),
            ))
    }

    fn render_reader(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self.reader_theme.palette();
        let Some(document) = self.document.as_ref() else {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_4()
                .bg(rgb(0xf4f2ec))
                .text_color(rgb(0x28271f))
                .child(
                    div()
                        .w(px(70.0))
                        .h(px(70.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgb(0xf4e322))
                        .text_size(px(32.0))
                        .child("▣"),
                )
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_size(px(22.0))
                        .child("Excel 文档阅读器"),
                )
                .child(div().text_color(rgb(0x737169)).child(
                    "按章节目录浏览长文档，使用全文搜索快速定位内容，并直接查看单元格中的图片。",
                ))
                .child(primary_button(
                    "选择 Excel 文件",
                    cx.listener(|this, _, window, cx| this.open_workbook(window, cx)),
                ))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(0x737169))
                        .child("支持 .xlsx / .xls / .xlsb / .ods / .csv，Ctrl+O 打开，Ctrl+F 搜索"),
                );
        };
        let Some(sheet) = document.sheets.get(self.sheet_index) else {
            unreachable!()
        };
        let query = self.reader_search.read(cx).value().to_lowercase();
        let outline_filter = self.reader_outline_filter.read(cx).value().to_lowercase();
        let matches = self.search_matches(cx);
        let section_match_counts = section_match_counts(&sheet.outline, &matches);
        let section_levels: HashMap<usize, usize> = sheet
            .outline
            .iter()
            .map(|item| (item.row_number, item.level))
            .collect();
        let data_rows: Vec<_> = sheet
            .rows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != sheet.header_row)
            .map(|(_, row)| row)
            .collect();
        let pages = data_rows.len().div_ceil(PAGE_ROWS).max(1);
        let page = self.reader_page.min(pages - 1);
        div()
            .flex()
            .flex_1()
            .min_h_0()
            .flex_col()
            .bg(rgb(palette.canvas))
            .text_color(rgb(palette.text))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(if self.sidebar_open { self.sidebar_width } else { 0.0 }))
                            .min_h_0()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_2()
                            .bg(rgb(0xebe9e2))
                            .text_color(rgb(0x28271f))
                            .border_1()
                            .border_color(rgb(0xc5c1b6))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(rgb(0x8b877d))
                                            .child("工作表"),
                                    )
                                    .child(
                                        div().flex().flex_wrap().gap_1().children(
                                            document.sheets.iter().enumerate().map(|(index, sheet)| {
                                                nav_button(
                                                    sheet.name.clone(),
                                                    index == self.sheet_index,
                                                    cx.listener(move |this, _, _, cx| this.select_sheet(index, cx)),
                                                )
                                            }),
                                        ),
                                    ),
                            )
                            .child(div().flex().items_center().justify_between()
                                .child(panel_title("文内搜索"))
                                .child(div().flex().gap_1()
                                    .child(button("◀", cx.listener(|this, _, _, cx| this.step_search(-1, cx))))
                                    .child(button("▶", cx.listener(|this, _, _, cx| this.step_search(1, cx)))))
                            )
                            .child(
                                div()
                                    .flex()
                                    .child(Input::new(&self.reader_search).flex_1())
                                    .children((!query.is_empty()).then(|| {
                                        button("清除", cx.listener(|this, _, window, cx| {
                                            this.reader_search.update(cx, |input, cx| {
                                                input.set_value("", window, cx)
                                            });
                                            this.search_cursor = None;
                                            this.active_cell = None;
                                            cx.notify();
                                        }))
                                    })),
                            )
                            .child(div().flex().gap_1()
                                .child(nav_button(format!("章节目录 ({})", sheet.outline.len()), self.sidebar_tab == SidebarTab::Outline,
                                    cx.listener(|this, _, _, cx| { this.sidebar_tab = SidebarTab::Outline; cx.notify(); })))
                                .child(nav_button(format!("搜索结果 ({})", matches.len()), self.sidebar_tab == SidebarTab::Search,
                                    cx.listener(|this, _, _, cx| { this.sidebar_tab = SidebarTab::Search; cx.notify(); }))))
                            .children((self.sidebar_tab == SidebarTab::Outline).then(||
                                div().flex_1().min_h_0().flex().flex_col().gap_1()
                                    .child(div().flex().items_center().justify_between()
                                        .child(panel_title("章节目录"))
                                        .child(div().flex().gap_1()
                                            .child(button("全部折叠", cx.listener(|this, _, _, cx| {
                                                if let Some(sheet) = this.document.as_ref().and_then(|d| d.sheets.get(this.sheet_index)) {
                                                    this.collapsed_sections = sheet.outline.iter().enumerate()
                                                        .filter(|(i, item)| sheet.outline.get(i + 1).is_some_and(|next| next.level > item.level))
                                                        .map(|(_, item)| item.row_number).collect();
                                                }
                                                cx.notify();
                                            })))
                                            .child(button("全部展开", cx.listener(|this, _, _, cx| {
                                                this.collapsed_sections.clear(); cx.notify();
                                            })))))
                                    .child(div().flex().gap_1().child(Input::new(&self.reader_outline_filter))
                                        .child(button(format!("{}级", if self.outline_depth == 0 { "全部".to_string() } else { self.outline_depth.to_string() }),
                                            cx.listener(|this, _, _, cx| {
                                                this.outline_depth = match this.outline_depth { 0 => 3, 3 => 5, 5 => 10, 10 => 20, _ => 0 };
                                                cx.notify();
                                            })))
                                    )
                                    .child(
                                div().h(px(220.0)).overflow_y_scrollbar().children(
                                    sheet
                                        .outline
                                        .iter()
                                        .filter(|item| {
                                            (self.outline_depth == 0 || item.level <= self.outline_depth)
                                                && (outline_filter.trim().is_empty()
                                                || format!("{} {}", item.number, item.title)
                                                    .to_lowercase()
                                                    .contains(outline_filter.as_str()))
                                                && ( !outline_filter.trim().is_empty() || !sheet.outline.iter()
                                                    .any(|parent| parent.row_number < item.row_number
                                                        && parent.level < item.level
                                                        && self.collapsed_sections.contains(&parent.row_number)
                                                        && !sheet.outline.iter().any(|next| next.row_number > parent.row_number
                                                            && next.row_number < item.row_number && next.level <= parent.level)))
                                        })
                                        .map(|item| {
                                            let row_number = item.row_number;
                                            let selected =
                                                self.selected_outline_row == Some(row_number);
                                            let count = section_match_counts.get(&row_number).copied().unwrap_or(0);
                                            let label = format!("{} {}{}", item.number, item.title,
                                                if count > 0 { format!(" · {count}") } else { String::new() });
                                            let has_children = sheet.outline.iter().find(|next| next.row_number > row_number)
                                                .is_some_and(|next| next.level > item.level);
                                            div()
                                                .id(ElementId::Name(
                                                    format!("outline-{row_number}").into(),
                                                ))
                                                .flex().items_center()
                                                .pl(px((item.level.saturating_sub(1) * 14 + 8) as f32))
                                                .bg(rgb(if selected { 0xf7f4df } else { 0xebe9e2 }))
                                                .text_size(px(12.0))
                                                .child(div().id(ElementId::Name(format!("outline-toggle-{row_number}").into()))
                                                    .w(px(18.0)).cursor_pointer()
                                                    .child(if has_children {
                                                        if self.collapsed_sections.contains(&row_number) { "▸" } else { "▾" }
                                                    } else { " " })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if has_children {
                                                        if !this.collapsed_sections.insert(row_number) { this.collapsed_sections.remove(&row_number); }
                                                        cx.notify();
                                                        }
                                                        cx.stop_propagation();
                                                    })))
                                                .child(div().id(ElementId::Name(format!("outline-link-{row_number}").into()))
                                                    .flex_1().p_1().cursor_pointer().child(label)
                                                    .on_click(cx.listener(move |this, _, _, cx| this.go_to_cell(row_number, None, cx))))
                                        }),
                                ),
                            )
                            ))
                            .children((self.sidebar_tab == SidebarTab::Search).then(||
                            div().flex_1().min_h_0().flex().flex_col()
                            .child(panel_title(format!("全部命中条目 · {} 条", matches.len())))
                            .child(div().flex_1().min_h_0().overflow_y_scrollbar().children(
                                matches.iter().enumerate().map(|(match_index, &(row_number, column))| {
                                    let excerpt = sheet.rows.iter().find(|row| row.row_number == row_number)
                                        .and_then(|row| row.cells.get(column)).cloned().unwrap_or_default();
                                    let excerpt: String = excerpt.chars().take(80).collect();
                                    div()
                                        .id(ElementId::Name(format!("match-{row_number}-{column}").into()))
                                        .p_1()
                                        .bg(rgb(if self.search_cursor == Some(match_index) { 0xf7f4df } else { 0xebe9e2 }))
                                        .cursor_pointer()
                                        .text_size(px(12.0))
                                        .child(format!("{}{} · {excerpt}", column_letters(column), row_number))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.search_cursor = Some(match_index);
                                            this.go_to_cell(row_number, Some(column), cx);
                                        }))
                                }),
                            ))
                            )),
                    )
                    .children(self.sidebar_open.then(|| div().id("sidebar-resizer")
                        .w(px(6.0)).h_full().bg(rgb(palette.grid)).cursor_pointer()
                        .on_mouse_down(gpui::MouseButton::Left,
                            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                this.dragging_sidebar = Some((event.position.x, this.sidebar_width));
                                cx.stop_propagation();
                            }))))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(self.render_reader_controls(sheet, cx))
                            .children(self.column_picker_open.then(||
                                div().flex().flex_wrap().gap_1()
                                    .child(button("显示全部列", cx.listener(|this, _, _, cx| {
                                        this.visible_columns.fill(true);
                                        cx.notify();
                                    })))
                                    .children(sheet.headers.iter().enumerate().map(|(index, name)| {
                                        let visible = self.visible_columns.get(index).copied().unwrap_or(true);
                                        nav_button(name.clone(), visible,
                                            cx.listener(move |this, _, _, cx| this.toggle_column(index, cx)))
                                    }))
                            ))
                            .children(document.warnings.iter().take(1).map(|warning|
                                div().p_1().bg(rgb(0xfff0d4)).text_color(rgb(0x86571b))
                                    .text_size(px(12.0)).child(warning.clone())))
                            .child(
                                div()
                                    .id("reader-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_scroll()
                                    .track_scroll(&self.reader_scroll)
                                    .flex()
                                    .flex_col()
                                    .bg(rgb(palette.cell))
                                    .border_1()
                                    .border_color(rgb(palette.grid))
                                    .child(reader_row(
                                                &sheet.headers,
                                                &self.visible_columns,
                                                true,
                                                self.reader_zoom as f32 / 100.0 * 10.4,
                                                false,
                                                0,
                                                self.sheet_index,
                                                &self.reader_images,
                                                &self.reader_image_names,
                                                &self.column_widths,
                                                self.reader_theme,
                                                self.reader_wrap,
                                                self.reader_compact,
                                                self.selected_outline_row,
                                                self.active_cell,
                                                &query,
                                                self.syntax_highlight,
                                                None,
                                                cx,
                                            ))
                                            .children(
                                                data_rows
                                                    .iter()
                                                    .skip(page * PAGE_ROWS)
                                                    .take(PAGE_ROWS)
                                                    .enumerate()
                                                    .map(|(index, row)| {
                                                        reader_row(
                                                            &row.cells,
                                                            &self.visible_columns,
                                                            false,
                                                            self.reader_zoom as f32 / 100.0 * 11.0,
                                                            index % 2 == 0,
                                                            row.row_number,
                                                            self.sheet_index,
                                                            &self.reader_images,
                                                            &self.reader_image_names,
                                                            &self.column_widths,
                                                            self.reader_theme,
                                                            self.reader_wrap,
                                                            self.reader_compact,
                                                            self.selected_outline_row,
                                                            self.active_cell,
                                                            &query,
                                                            self.syntax_highlight,
                                                            section_levels.get(&row.row_number).copied(),
                                                            cx,
                                                        )
                                                    }),
                                            ),
                            )
                            .child(div().flex().items_center().justify_between().text_size(px(11.0))
                                .text_color(rgb(palette.row_number))
                                .child(format!("{} 行 · {} 列 · {} 张图片",
                                    sheet.total_rows, sheet.total_columns, sheet.images.len()))
                                .child(div().flex().items_center().gap_1()
                                    .child(button("上一页", cx.listener(|this, _, _, cx| {
                                        this.reader_page = this.reader_page.saturating_sub(1);
                                        cx.notify();
                                    })))
                                    .child(format!("{} / {}", page + 1, pages))
                                    .child(button("下一页", cx.listener(move |this, _, _, cx| {
                                        this.reader_page = (this.reader_page + 1).min(pages - 1);
                                        cx.notify();
                                    }))))
                                .child("拖动表头边界调整列宽")),
                    ),
            )
    }
}

impl Render for StructSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(column) = self.pending_scroll_column.take() {
            cx.on_next_frame(window, move |this, _, cx| {
                this.reveal_column(column);
                cx.notify();
            });
        }
        div()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.page == Page::Reader {
                    let key = event.keystroke.key.to_lowercase();
                    if event.keystroke.modifiers.control && key == "o" {
                        this.open_workbook(window, cx);
                        cx.stop_propagation();
                    } else if event.keystroke.modifiers.control && key == "f" {
                        this.sidebar_open = true;
                        this.sidebar_tab = SidebarTab::Search;
                        this.reader_search
                            .update(cx, |input, cx| input.focus(window, cx));
                        cx.stop_propagation();
                        cx.notify();
                    } else if key == "escape" {
                        if this.active_image.is_some() {
                            this.active_image = None;
                        } else if this.column_picker_open {
                            this.column_picker_open = false;
                        } else {
                            window.remove_window();
                        }
                        cx.stop_propagation();
                        cx.notify();
                    } else if key == "enter" && this.sidebar_tab == SidebarTab::Search {
                        this.step_search(
                            if event.keystroke.modifiers.shift {
                                -1
                            } else {
                                1
                            },
                            cx,
                        );
                        cx.stop_propagation();
                    }
                } else if this.macro_open && event.keystroke.key.to_lowercase() == "escape" {
                    this.macro_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some(drag) = this.dragging_title {
                        title_drag_update(window, drag, event.position);
                    }
                    if let Some((column, start_x, start_width)) = this.dragging_column {
                        if let Some(width) = this.column_widths.get_mut(column) {
                            *width = (start_width + f32::from(event.position.x - start_x))
                                .clamp(64.0, 760.0);
                            cx.notify();
                        }
                    }
                    if let Some((start_x, start_width)) = this.dragging_sidebar {
                        this.sidebar_width = (start_width + f32::from(event.position.x - start_x))
                            .clamp(220.0, 620.0);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if this.dragging_title.take().is_some() {
                        title_drag_end();
                    }
                    this.dragging_column = None;
                    if this.dragging_sidebar.is_some() {
                        this.save_reader_prefs();
                    }
                    this.dragging_sidebar = None;
                }),
            )
            .flex()
            .flex_col()
            .bg(rgb(if self.page == Page::Reader {
                self.reader_theme.palette().canvas
            } else {
                0xf4f2ec
            }))
            .text_color(rgb(if self.page == Page::Reader {
                self.reader_theme.palette().text
            } else {
                0x28271f
            }))
            .child(self.render_title_bar(window, cx))
            .child(if self.page == Page::Reader {
                self.render_reader_toolbar(cx).into_any_element()
            } else {
                self.render_toolbar(cx).into_any_element()
            })
            .child(match self.page {
                Page::Struct => self.render_struct(cx).into_any_element(),
                Page::Reader => self.render_reader(cx).into_any_element(),
            })
            .children(
                (self.error
                    || self.status.starts_with("已导出")
                    || self.status.starts_with("已保存")
                    || self.status.starts_with("代码已复制"))
                .then(|| {
                    div()
                        .p_2()
                        .bg(rgb(if self.error { 0xffe4df } else { 0xfff9d8 }))
                        .text_size(px(12.0))
                        .child(self.status.clone())
                }),
            )
            .children(self.macro_open.then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgba(0x24231fa8))
                    .child(
                        div()
                            .w(px(880.0))
                            .h(px(710.0))
                            .flex()
                            .flex_col()
                            .bg(rgb(0xfbfaf6))
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(0xdcd8cc))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .p_3()
                                    .border_b_1()
                                    .border_color(rgb(0xdcd8cc))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap_3()
                                            .child(
                                                div()
                                                    .w(px(40.0))
                                                    .h(px(40.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .bg(rgb(0xf4e322))
                                                    .text_size(px(20.0))
                                                    .child("▦"),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .child(
                                                        div()
                                                            .text_size(px(10.0))
                                                            .text_color(rgb(0x737169))
                                                            .child("INTEGRATED TOOL"),
                                                    )
                                                    .child(panel_title("表格转 X-Macro")),
                                            ),
                                    )
                                    .child(button(
                                        "×",
                                        cx.listener(|this, _, _, cx| {
                                            this.macro_open = false;
                                            cx.notify();
                                        }),
                                    )),
                            )
                            .child(self.render_macro(cx)),
                    )
            }))
            .children(self.active_image.iter().map(|(image, name, row, column)| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .flex_col()
                    .bg(gpui::rgba(0x0c1724e8))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .p_3()
                            .bg(rgb(0x172634))
                            .text_color(rgb(0xffffff))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(name.clone()),
                            )
                            .child(format!("第 {row} 行 · {} 列", column_letters(*column)))
                            .child(div().flex_1())
                            .child(button(
                                "−",
                                cx.listener(|this, _, _, cx| {
                                    this.image_zoom = (this.image_zoom - 0.25).max(0.25);
                                    cx.notify();
                                }),
                            ))
                            .child(button(
                                format!("{}%", (self.image_zoom * 100.0) as u32),
                                cx.listener(|this, _, _, cx| {
                                    this.image_zoom = 1.0;
                                    cx.notify();
                                }),
                            ))
                            .child(button(
                                "+",
                                cx.listener(|this, _, _, cx| {
                                    this.image_zoom = (this.image_zoom + 0.25).min(4.0);
                                    cx.notify();
                                }),
                            ))
                            .child(button(
                                "关闭图片",
                                cx.listener(|this, _, _, cx| {
                                    this.active_image = None;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .overflow_scrollbar()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                img(image.clone())
                                    .w(px(800.0 * self.image_zoom))
                                    .h(px(600.0 * self.image_zoom))
                                    .object_fit(gpui::ObjectFit::Contain),
                            ),
                    )
            }))
    }
}

fn export_stem(name: &str) -> String {
    let upper = name.trim().to_ascii_uppercase();
    match upper.as_str() {
        "TR2_DD_IN" | "TR2_SW_DD_IN" => "TR2_SW_DD_IN".into(),
        "TR2_DD_OUT" | "TR2_SW_DD_OUT" => "TR2_SW_DD_OUT".into(),
        _ => name.trim().to_string(),
    }
}

fn column_letters(mut index: usize) -> String {
    let mut result = String::new();
    loop {
        result.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    result
}

fn section_match_counts(
    outline: &[excel_reader::OutlineItem],
    matches: &[(usize, usize)],
) -> HashMap<usize, usize> {
    let mut counts = HashMap::new();
    let mut section_index = 0usize;
    for &(row, _) in matches {
        while section_index + 1 < outline.len() && outline[section_index + 1].row_number <= row {
            section_index += 1;
        }
        if outline
            .get(section_index)
            .is_some_and(|section| section.row_number <= row)
        {
            let mut parent_level = usize::MAX;
            for section in outline[..=section_index].iter().rev() {
                if section.level < parent_level {
                    *counts.entry(section.row_number).or_insert(0) += 1;
                    parent_level = section.level;
                    if parent_level == 1 {
                        break;
                    }
                }
            }
        }
    }
    counts
}

fn suggest_column_widths(sheet: &excel_reader::ExcelSheet) -> Vec<f32> {
    sheet
        .headers
        .iter()
        .enumerate()
        .map(|(column, header)| {
            let mut longest = header.chars().count().max(4);
            for row in sheet.rows.iter().take(300) {
                let length = row
                    .cells
                    .get(column)
                    .map(|cell| {
                        cell.lines()
                            .map(|line| line.chars().count())
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0);
                longest = longest.max(length);
            }
            let mut width = (longest as f32 * 7.0 + 28.0).clamp(76.0, 420.0);
            let lower = header.to_lowercase();
            if ["需求", "content", "description", "comment", "说明"]
                .iter()
                .any(|term| lower.contains(term))
            {
                width = width.max(420.0);
            }
            if ["标题编号", "section", "编号"]
                .iter()
                .any(|term| lower.contains(term))
            {
                width = width.clamp(120.0, 190.0);
            }
            if lower == "id" || lower.contains("序号") {
                width = width.min(90.0);
            }
            if sheet.images.iter().any(|image| image.column == column) {
                width = width.max(220.0);
            }
            width
        })
        .collect()
}

fn panel_title(label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .font_weight(gpui::FontWeight::BOLD)
        .text_size(px(15.0))
        .child(label.into())
}

fn button(
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(ElementId::Name(label.clone()))
        .px_3()
        .py_2()
        .bg(rgb(0xffffff))
        .text_color(rgb(0x28271f))
        .border_1()
        .border_color(rgb(0xdcd8cc))
        .cursor_pointer()
        .text_size(px(12.0))
        .child(label)
        .on_click(on_click)
}

fn primary_button(
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(ElementId::Name(label.clone()))
        .px_3()
        .py_2()
        .bg(rgb(0xf4e322))
        .text_color(rgb(0x28271f))
        .border_1()
        .border_color(rgb(0xe3cf00))
        .cursor_pointer()
        .text_size(px(12.0))
        .child(label)
        .on_click(on_click)
}

fn dark_button(
    label: impl Into<SharedString>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(ElementId::Name(label.clone()))
        .px_3()
        .py_2()
        .bg(rgb(0x292820))
        .text_color(rgb(0xffffff))
        .cursor_pointer()
        .text_size(px(12.0))
        .child(label)
        .on_click(on_click)
}

fn disabled_button(label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px_3()
        .py_2()
        .bg(rgb(0xf7ed9a))
        .text_color(rgb(0x918b5b))
        .text_size(px(12.0))
        .child(label.into())
}

fn nav_button(
    label: impl Into<SharedString>,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(ElementId::Name(label.clone()))
        .px_2()
        .py_1()
        .cursor_pointer()
        .text_size(px(12.0))
        .bg(rgb(if selected { 0xf4e322 } else { 0xefede6 }))
        .text_color(rgb(0x28271f))
        .child(label)
        .on_click(on_click)
}

fn field_row(cells: [&str; 8], header: bool, shaded: bool) -> impl IntoElement {
    let widths = [105.0, 220.0, 245.0, 145.0, 100.0, 125.0, 150.0, 440.0];
    div()
        .flex()
        .bg(rgb(if header {
            0xf4e322
        } else if shaded {
            0xf8f7f2
        } else {
            0xffffff
        }))
        .border_b_1()
        .border_color(rgb(0xdcd8cc))
        .children(
            cells
                .iter()
                .enumerate()
                .zip(widths)
                .map(|((index, text), width)| {
                    if index == 0 && !header {
                        div()
                            .w(px(width))
                            .min_w(px(width))
                            .p_2()
                            .border_r_1()
                            .border_color(rgb(0xdcd8cc))
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(rgb(0xe7f5ed))
                                    .text_color(rgb(0x287a5b))
                                    .text_size(px(11.0))
                                    .child(format!("✓ {text}")),
                            )
                            .into_any_element()
                    } else {
                        table_cell((*text).to_string(), width, 12.0).into_any_element()
                    }
                }),
        )
}

fn reader_row(
    cells: &[String],
    visible: &[bool],
    header: bool,
    font_size: f32,
    shaded: bool,
    row_number: usize,
    sheet_index: usize,
    images: &HashMap<(usize, usize, usize), Arc<Image>>,
    image_names: &HashMap<(usize, usize, usize), String>,
    widths: &[f32],
    theme: ReaderTheme,
    wrap: bool,
    compact: bool,
    selected_row: Option<usize>,
    active_cell: Option<(usize, usize)>,
    query: &str,
    syntax: bool,
    section_level: Option<usize>,
    cx: &mut Context<StructSheet>,
) -> impl IntoElement {
    let row_height = if compact {
        28.0
    } else if header {
        48.0
    } else {
        46.0
    };
    let palette = theme.palette();
    let row_background = if header {
        palette.header
    } else if selected_row == Some(row_number) {
        palette.selected
    } else if section_level.is_some() {
        palette.section
    } else if shaded {
        palette.canvas
    } else {
        palette.cell
    };
    div()
        .id(ElementId::Name(
            format!("reader-row-{sheet_index}-{row_number}").into(),
        ))
        .flex()
        .min_h(px(row_height))
        .bg(rgb(row_background))
        .text_color(rgb(palette.text))
        .border_b_1()
        .border_color(rgb(palette.grid))
        .on_click(cx.listener(move |this, _, _, cx| {
            if !header {
                this.selected_outline_row = Some(row_number);
                cx.notify();
            }
        }))
        .child(
            div()
                .w(px(48.0))
                .min_w(px(48.0))
                .p_2()
                .text_size(px(font_size * 0.85))
                .text_color(rgb(palette.row_number))
                .child(if header {
                    "#".to_string()
                } else {
                    row_number.to_string()
                }),
        )
        .children(
            cells
                .iter()
                .enumerate()
                .filter(|(index, _)| visible.get(*index).copied().unwrap_or(true))
                .map(|(column, text)| {
                    let matching = !query.is_empty() && text.to_lowercase().contains(query);
                    let active = active_cell == Some((row_number, column));
                    let width = widths.get(column).copied().unwrap_or(180.0);
                    let cell = div()
                        .id(ElementId::Name(
                            format!("reader-cell-{sheet_index}-{row_number}-{column}").into(),
                        ))
                        .w(px(width))
                        .min_w(px(width))
                        .p_1()
                        .px_2()
                        .bg(rgb(if active {
                            palette.search_active
                        } else if matching {
                            palette.search
                        } else {
                            row_background
                        }))
                        .border_r_1()
                        .border_color(rgb(palette.grid))
                        .text_size(px(font_size))
                        .font_weight(if header || section_level.is_some() {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(rgb(palette.text))
                        .child(if wrap {
                            div()
                                .child(reader_text(text, query, syntax))
                                .into_any_element()
                        } else {
                            div()
                                .whitespace_nowrap()
                                .child(reader_text(text, query, syntax))
                                .into_any_element()
                        })
                        .children(header.then(|| {
                            div()
                                .id(ElementId::Name(format!("column-grip-{column}").into()))
                                .w(px(8.0))
                                .h(px(16.0))
                                .bg(rgb(palette.row_number))
                                .cursor_pointer()
                                .on_mouse_down(
                                    gpui::MouseButton::Left,
                                    cx.listener(
                                        move |this, event: &gpui::MouseDownEvent, _, cx| {
                                            if event.click_count >= 2 {
                                                if let Some(sheet) = this
                                                    .document
                                                    .as_ref()
                                                    .and_then(|d| d.sheets.get(this.sheet_index))
                                                {
                                                    if let Some(width) =
                                                        this.column_widths.get_mut(column)
                                                    {
                                                        *width = suggest_column_widths(sheet)
                                                            .get(column)
                                                            .copied()
                                                            .unwrap_or(120.0);
                                                    }
                                                }
                                                this.dragging_column = None;
                                            } else {
                                                let width = this
                                                    .column_widths
                                                    .get(column)
                                                    .copied()
                                                    .unwrap_or(120.0);
                                                this.dragging_column =
                                                    Some((column, event.position.x, width));
                                            }
                                            cx.stop_propagation();
                                            cx.notify();
                                        },
                                    ),
                                )
                        }));
                    if let Some(image) = images.get(&(sheet_index, row_number, column)) {
                        let selected = image.clone();
                        let name = image_names
                            .get(&(sheet_index, row_number, column))
                            .cloned()
                            .unwrap_or_else(|| "图片".into());
                        cell.child(
                            div()
                                .id(ElementId::Name(
                                    format!("image-{sheet_index}-{row_number}-{column}").into(),
                                ))
                                .cursor_pointer()
                                .child(
                                    img(image.clone())
                                        .w(px(160.0))
                                        .h(px(100.0))
                                        .object_fit(gpui::ObjectFit::Contain),
                                )
                                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                                    if event.click_count() >= 2 {
                                        this.active_image = Some((
                                            selected.clone(),
                                            name.clone(),
                                            row_number,
                                            column,
                                        ));
                                        this.image_zoom = 1.0;
                                        cx.notify();
                                    }
                                })),
                        )
                    } else {
                        cell
                    }
                }),
        )
}

fn reader_text(text: &str, query: &str, syntax: bool) -> StyledText {
    static SYNTAX: OnceLock<Regex> = OnceLock::new();
    let mut spans = Vec::new();
    if syntax {
        let regex = SYNTAX.get_or_init(|| Regex::new(r#"(?i)\[[A-Z][A-Z0-9_-]*\]|<[^>\r\n]+>|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\b(?:shall|must|should|if|then|else|when|while|and|or|not|true|false|null|return|unsigned|signed|char|short|int|long|float|double|bool|void|uint\d+_t|int\d+_t|0x[\da-f]+|\d+(?:\.\d+)*)\b"#).expect("syntax regex"));
        for token in regex.find_iter(text) {
            let value = token.as_str();
            let color = if value.starts_with('[') {
                0x287d9e
            } else if value.starts_with('<') {
                0x7853a4
            } else if value.starts_with('"') || value.starts_with('\'') {
                0x2a8753
            } else if value.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                0xb06d2a
            } else {
                0x9d4988
            };
            spans.push((
                token.range(),
                HighlightStyle {
                    color: Some(rgb(color).into()),
                    ..Default::default()
                },
            ));
        }
    }
    if !query.is_empty() {
        let lower = text.to_lowercase();
        for (index, _) in lower.match_indices(query) {
            let end = (index + query.len()).min(text.len());
            if text.is_char_boundary(index) && text.is_char_boundary(end) {
                spans.push((
                    index..end,
                    HighlightStyle {
                        color: Some(rgb(0x2b2617).into()),
                        background_color: Some(rgb(0xffda75).into()),
                        font_weight: Some(gpui::FontWeight::BOLD),
                        ..Default::default()
                    },
                ));
            }
        }
    }
    StyledText::new(text.to_string()).with_highlights(spans)
}

fn decode_images(
    document: &excel_reader::ExcelDocument,
) -> HashMap<(usize, usize, usize), Arc<Image>> {
    let mut decoded = HashMap::new();
    for (sheet_index, sheet) in document.sheets.iter().enumerate() {
        for picture in &sheet.images {
            let Some(format) = ImageFormat::from_mime_type(&picture.mime_type) else {
                continue;
            };
            let Some((_, encoded)) = picture.data_url.split_once(',') else {
                continue;
            };
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
                continue;
            };
            decoded.insert(
                (sheet_index, picture.row, picture.column),
                Arc::new(Image::from_bytes(format, bytes)),
            );
        }
    }
    decoded
}

fn table_cell(text: String, width: f32, font_size: f32) -> impl IntoElement {
    div()
        .w(px(width))
        .min_w(px(width))
        .p_2()
        .border_r_1()
        .border_color(rgb(0xdcd8cc))
        .text_size(px(font_size))
        .child(text)
}

fn title_control(label: &'static str, area: WindowControlArea) -> impl IntoElement {
    div()
        .id(label)
        .w(px(46.0))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .on_click(move |_, window, _| match area {
            WindowControlArea::Min => window.minimize_window(),
            WindowControlArea::Max => toggle_maximize(window),
            WindowControlArea::Close => window.remove_window(),
            _ => {}
        })
        .text_size(px(17.0))
        .text_color(rgb(0x28271f))
        .hover(|style| style.bg(rgb(if label == "×" { 0xe95050 } else { 0xe9e7df })))
        .child(label)
}

fn toggle_maximize(window: &mut Window) {
    if !window.is_maximized() {
        window.zoom_window();
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(handle) = HasWindowHandle::window_handle(window) {
            if let RawWindowHandle::Win32(handle) = handle.as_raw() {
                #[link(name = "user32")]
                extern "system" {
                    fn PostMessageW(
                        hwnd: *mut std::ffi::c_void,
                        message: u32,
                        wparam: usize,
                        lparam: isize,
                    ) -> i32;
                }
                unsafe {
                    PostMessageW(
                        handle.hwnd.get() as *mut std::ffi::c_void,
                        0x0112,
                        0xF120,
                        0,
                    )
                };
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    window.zoom_window();
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct NativeRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(target_os = "windows")]
fn native_hwnd(window: &Window) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
}

fn title_drag_start(window: &Window, position: gpui::Point<gpui::Pixels>) -> Option<TitleDrag> {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn SetCapture(hwnd: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        }
        let hwnd = native_hwnd(window)?;
        unsafe { SetCapture(hwnd) };
        Some(TitleDrag {
            start_x: f32::from(position.x),
            start_y: f32::from(position.y),
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = position;
        window.start_window_move();
        None
    }
}

fn title_drag_update(window: &Window, drag: TitleDrag, position: gpui::Point<gpui::Pixels>) {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetWindowRect(hwnd: *mut std::ffi::c_void, rect: *mut NativeRect) -> i32;
            fn SetWindowPos(
                hwnd: *mut std::ffi::c_void,
                after: *mut std::ffi::c_void,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                flags: u32,
            ) -> i32;
        }
        if let Some(hwnd) = native_hwnd(window) {
            let mut rect = NativeRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            unsafe {
                if GetWindowRect(hwnd, &mut rect) != 0 {
                    let dx = (f32::from(position.x) - drag.start_x) * window.scale_factor();
                    let dy = (f32::from(position.y) - drag.start_y) * window.scale_factor();
                    let x = rect.left + dx.round() as i32;
                    let y = rect.top + dy.round() as i32;
                    SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, 0x0015);
                }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (window, drag, position);
}

fn title_drag_end() {
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn ReleaseCapture() -> i32;
        }
        unsafe { ReleaseCapture() };
    }
}

pub fn run() {
    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);
        let mut code_theme = (*gpui_component::highlighter::HighlightTheme::default_dark()).clone();
        code_theme.style.editor_background = Some(rgb(0x25251f).into());
        code_theme.style.editor_foreground = Some(rgb(0xf5f2e8).into());
        code_theme.style.editor_active_line = Some(rgb(0x33332c).into());
        code_theme.style.editor_line_number = Some(rgb(0x928f83).into());
        code_theme.style.editor_active_line_number = Some(rgb(0xf4e322).into());
        gpui_component::theme::Theme::global_mut(cx).highlight_theme = Arc::new(code_theme);
        let bounds = Bounds::centered(None, size(px(1240.0), px(790.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitleBar::title_bar_options()),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("StructSheet · C 结构体转 Excel");
                let view = cx.new(|cx| StructSheet::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("无法打开 StructSheet 窗口");
    });
}
