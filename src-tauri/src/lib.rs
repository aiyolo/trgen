mod excel;
#[cfg(windows)]
mod file_dialog;
#[cfg(windows)]
mod native_splash;
mod parser;
mod table_xmacro;

use parser::{ParseResult, StructCatalog};
use std::path::PathBuf;

#[tauri::command]
fn parse_source(source: String, root_name: Option<String>) -> Result<ParseResult, String> {
    parser::parse(&source, root_name.as_deref())
}

#[tauri::command]
fn list_structs(source: String) -> Result<StructCatalog, String> {
    parser::discover(&source)
}

#[tauri::command]
fn export_excel(source: String, root_name: Option<String>, path: String) -> Result<String, String> {
    let result = parser::parse(&source, root_name.as_deref())?;
    let mut output_path = PathBuf::from(path);
    if output_path.extension().is_none() {
        output_path.set_extension("xlsx");
    }
    excel::write_workbook(&output_path, &result)?;
    Ok(output_path.to_string_lossy().into_owned())
}

#[tauri::command]
fn convert_table_to_xmacro(
    path: String,
    clipboard_text: String,
    macro_name: String,
    struct_name: String,
) -> Result<table_xmacro::ResultData, String> {
    table_xmacro::convert(path, clipboard_text, macro_name, struct_name)
}

#[tauri::command]
fn save_text_file(path: String, content: String) -> Result<String, String> {
    let mut output_path = PathBuf::from(path);
    if output_path.extension().is_none() {
        output_path.set_extension("h");
    }
    std::fs::write(&output_path, content).map_err(|error| format!("代码文件保存失败：{error}"))?;
    Ok(output_path.to_string_lossy().into_owned())
}

#[cfg(windows)]
#[tauri::command]
fn choose_export_path(
    window: tauri::WebviewWindow,
    default_name: String,
) -> Result<Option<String>, String> {
    file_dialog::choose_excel_path(&window, &default_name)
}

#[cfg(windows)]
#[tauri::command]
fn choose_table_path(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    file_dialog::choose_table_path(&window)
}

#[cfg(windows)]
#[tauri::command]
fn choose_code_path(
    window: tauri::WebviewWindow,
    default_name: String,
) -> Result<Option<String>, String> {
    file_dialog::choose_code_path(&window, &default_name)
}

#[cfg(not(windows))]
#[tauri::command]
fn choose_export_path(
    _window: tauri::WebviewWindow,
    _default_name: String,
) -> Result<Option<String>, String> {
    Err("当前平台暂不支持保存窗口。".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn choose_table_path(_window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    Err("当前平台暂不支持文件选择窗口。".to_string())
}

#[cfg(not(windows))]
#[tauri::command]
fn choose_code_path(
    _window: tauri::WebviewWindow,
    _default_name: String,
) -> Result<Option<String>, String> {
    Err("当前平台暂不支持保存窗口。".to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    configure_portable_webview2();

    #[cfg(windows)]
    let splash = std::sync::Arc::new(std::sync::Mutex::new(native_splash::NativeSplash::show()));
    #[cfg(windows)]
    let splash_on_load = splash.clone();

    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![
        list_structs,
        parse_source,
        export_excel,
        choose_export_path,
        convert_table_to_xmacro,
        choose_table_path,
        choose_code_path,
        save_text_file
    ]);

    #[cfg(windows)]
    let builder = builder.on_page_load(move |webview, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            let window = webview.window();
            let _ = window.center();
            let _ = window.show();
            if let Ok(mut splash) = splash_on_load.lock() {
                if let Some(splash) = splash.take() {
                    splash.close();
                }
            }
        }
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running StructSheet");
}

#[cfg(windows)]
fn configure_portable_webview2() {
    let Some(executable_dir) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let runtime_dir = executable_dir.join("WebView2Runtime");
    if runtime_dir.join("msedgewebview2.exe").is_file() {
        std::env::set_var("WEBVIEW2_BROWSER_EXECUTABLE_FOLDER", runtime_dir);
    }
}
