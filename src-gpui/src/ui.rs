use crate::{excel, parser, table_xmacro};
use gpui::prelude::*;
use gpui::{
    div, img, px, rgb, size, App, Application, Bounds, ClickEvent, ClipboardItem, Context,
    ElementId, Entity, FocusHandle, FontFallbacks, Image, ImageFormat, IntoElement, Render,
    ScrollHandle, SharedString, StatefulInteractiveElement, Subscription, Window, WindowBounds,
    WindowControlArea, WindowOptions,
};
use gpui_component::{
    input::{Input, InputState},
    scroll::{ScrollableElement, Scrollbar, ScrollbarShow},
    select::{Select, SelectEvent, SelectState},
    Root, TitleBar,
};
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

const SAMPLE: &str = "typedef struct {\n    uint16_t channel;       // 通道编号\n    float voltage;          // 电压值\n    float temperature;      // 温度\n} SensorData;\n\ntypedef struct {\n    uint32_t device_id;     // 设备编号\n    uint8_t status;         // 设备状态\n    SensorData sensors[2];  // 两路传感器\n    char name[16];          // 设备名称\n} DevicePacket;";
// Match the original preview's column priorities at a compact minimum width.
const PREVIEW_COLUMN_WIDTHS: [f32; 8] = [52.0, 106.0, 138.0, 60.0, 56.0, 66.0, 92.0, 100.0];

const APP_VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy)]
struct TitleDrag {
    start_x: f32,
    start_y: f32,
}

struct StructSheet {
    focus_handle: FocusHandle,
    macro_open: bool,
    source: Entity<InputState>,
    macro_name: Entity<InputState>,
    clipboard_table: Entity<InputState>,
    root_select: Entity<SelectState<Vec<String>>>,
    selected_root: Option<String>,
    catalog: Vec<String>,
    parse_result: Option<parser::ParseResult>,
    macro_path: Option<PathBuf>,
    macro_result: Option<table_xmacro::ResultData>,
    macro_error: Option<String>,
    dragging_title: Option<TitleDrag>,
    preview_scroll: ScrollHandle,
    status: String,
    error: bool,
    _subscriptions: Vec<Subscription>,
}

impl StructSheet {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window);
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
                .soft_wrap(false)
                .placeholder("粘贴 Excel/WPS 中的单元格，或选择表格文件")
        });
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
        ];
        Self {
            focus_handle,
            macro_open: false,
            source,
            macro_name,
            clipboard_table,
            root_select,
            selected_root: None,
            catalog: Vec::new(),
            parse_result: None,
            macro_path: None,
            macro_result: None,
            macro_error: None,
            dragging_title: None,
            preview_scroll: ScrollHandle::new(),
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
                    )
                    .child(
                        div()
                            .px_2()
                            .py(px(2.0))
                            .rounded_full()
                            .bg(rgb(0xefede6))
                            .text_color(rgb(0x737169))
                            .text_size(px(11.0))
                            .child(APP_VERSION),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
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

    fn render_title_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = format!("StructSheet {APP_VERSION} · C 结构体转 Excel");
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
                    .child(img(app_icon()).w(px(18.0)).h(px(18.0)))
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
                            .min_w_0()
                            .relative()
                            .bg(rgb(0xffffff))
                            .border_1()
                            .border_color(rgb(0xdcd8cc))
                            .child(
                                div()
                                    .id("struct-preview-scroll")
                                    .size_full()
                                    .track_scroll(&self.preview_scroll)
                                    .overflow_scroll()
                                    .child(
                                        div()
                                            .w_full()
                                            .h_full()
                                            .min_w(px(if result.is_some() {
                                                PREVIEW_COLUMN_WIDTHS.into_iter().sum()
                                            } else {
                                                0.0
                                            }))
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
                            .children(result.map(|_| {
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .child(
                                        Scrollbar::new(&self.preview_scroll)
                                            .scrollbar_show(ScrollbarShow::Always),
                                    )
                            })),
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
            .min_w_0()
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
                                    .min_w_0()
                                    .p_2()
                                    .truncate()
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
                    .child(
                        Input::new(&self.clipboard_table)
                            .w_full()
                            .min_w_0()
                            .h(px(88.0)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
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
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .min_w_0()
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
                            .min_w_0()
                            .w_full()
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
}

impl Render for StructSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("app-root")
            .track_focus(&self.focus_handle)
            .size_full()
            .relative()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                if this.macro_open && event.keystroke.key.to_lowercase() == "escape" {
                    this.macro_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, _cx| {
                    if let Some(drag) = this.dragging_title {
                        title_drag_update(window, drag, event.position);
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if this.dragging_title.take().is_some() {
                        title_drag_end();
                    }
                }),
            )
            .flex()
            .flex_col()
            .bg(rgb(0xf4f2ec))
            .text_color(rgb(0x28271f))
            .font(ui_font())
            .child(self.render_title_bar(window, cx))
            .child(self.render_toolbar(cx).into_any_element())
            .child(self.render_struct(cx).into_any_element())
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
                let modal_width = (window.bounds().size.width - px(42.0)).min(px(880.0));
                let modal_height = (window.bounds().size.height - px(48.0)).min(px(710.0));
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
                            .w(modal_width)
                            .h(modal_height)
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .overflow_hidden()
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
                                    .child(
                                        div()
                                            .id("macro-close")
                                            .px_3()
                                            .py_2()
                                            .bg(rgb(0xffffff))
                                            .text_color(rgb(0x28271f))
                                            .border_1()
                                            .border_color(rgb(0xdcd8cc))
                                            .cursor_pointer()
                                            .text_size(px(12.0))
                                            .child("×")
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                cx.listener(|this, _, _, cx| {
                                                    this.macro_open = false;
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                }),
                                            ),
                                    ),
                            )
                            .child(self.render_macro(cx)),
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

fn field_row(cells: [&str; 8], header: bool, shaded: bool) -> impl IntoElement {
    let widths = PREVIEW_COLUMN_WIDTHS;
    div()
        .flex()
        .h(px(if header { 37.0 } else { 34.0 }))
        .bg(rgb(if header {
            0xf4e322
        } else if shaded {
            0xfbfaf6
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
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .p_1()
                            .border_r_1()
                            .border_color(rgb(0xdcd8cc))
                            .child(
                                div()
                                    .px_1()
                                    .py_1()
                                    .rounded_full()
                                    .bg(rgb(0xe7f5ed))
                                    .text_color(rgb(0x287a5b))
                                    .text_size(px(10.0))
                                    .truncate()
                                    .child(format!("✓ {text}")),
                            )
                            .into_any_element()
                    } else {
                        table_cell(
                            (*text).to_string(),
                            width,
                            header,
                            !header && matches!(index, 3..=6),
                            index == 7,
                        )
                        .into_any_element()
                    }
                }),
        )
}

fn table_cell(
    text: String,
    width: f32,
    header: bool,
    centered: bool,
    grow: bool,
) -> impl IntoElement {
    div()
        .w(px(width))
        .min_w(px(width))
        .when(grow, |this| this.flex_grow())
        .h_full()
        .flex()
        .items_center()
        .when(centered, |this| this.justify_center())
        .p_1()
        .border_r_1()
        .border_color(rgb(0xdcd8cc))
        .text_size(px(if header { 10.5 } else { 12.0 }))
        .truncate()
        .child(text)
}

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../icons/32x32.png").to_vec(),
        ))
    })
    .clone()
}

fn ui_font() -> gpui::Font {
    let mut font = gpui::font("Noto Sans SC");
    font.fallbacks = Some(FontFallbacks::from_fonts(vec![
        "Microsoft YaHei UI".into(),
        "Microsoft YaHei".into(),
    ]));
    font
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

#[cfg(target_os = "windows")]
fn set_native_window_icon(window: &Window) {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(module: *const u16) -> *mut std::ffi::c_void;
    }
    #[link(name = "user32")]
    extern "system" {
        fn LoadImageW(
            instance: *mut std::ffi::c_void,
            name: *const u16,
            image_type: u32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> *mut std::ffi::c_void;
        fn SendMessageW(
            hwnd: *mut std::ffi::c_void,
            message: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize;
    }
    let Some(hwnd) = native_hwnd(window) else {
        return;
    };
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        for (kind, size) in [(0, 16), (1, 32)] {
            let icon = LoadImageW(instance, 1usize as *const u16, 1, size, size, 0x8000);
            if !icon.is_null() {
                SendMessageW(hwnd, 0x80, kind, icon as isize);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn set_native_window_icon(_: &Window) {}

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
        gpui_component::theme::Theme::global_mut(cx).font_family = "Noto Sans SC".into();
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
                set_native_window_icon(window);
                let view = cx.new(|cx| StructSheet::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("无法打开 StructSheet 窗口");
    });
}
