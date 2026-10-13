#![cfg_attr(windows, windows_subsystem = "windows")]

#[path = "../../src-tauri/src/excel.rs"]
mod excel;
#[path = "../../src-tauri/src/excel_reader.rs"]
mod excel_reader;
#[path = "../../src-tauri/src/parser.rs"]
mod parser;
#[path = "../../src-tauri/src/table_xmacro.rs"]
mod table_xmacro;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Request {
    action: String,
    source: String,
    root_name: Option<String>,
    path: String,
    clipboard_text: String,
    macro_name: String,
    struct_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    ok: bool,
    data: Value,
    error: Option<String>,
}

fn run(request: Request) -> Result<Value, String> {
    match request.action.as_str() {
        "list_structs" => serde_json::to_value(parser::discover(&request.source)?)
            .map_err(|error| error.to_string()),
        "parse_source" => serde_json::to_value(parser::parse(
            &request.source,
            request.root_name.as_deref(),
        )?)
        .map_err(|error| error.to_string()),
        "export_excel" => {
            let result = parser::parse(&request.source, request.root_name.as_deref())?;
            let mut path = PathBuf::from(request.path);
            if path.extension().is_none() {
                path.set_extension("xlsx");
            }
            excel::write_workbook(&path, &result)?;
            Ok(json!(path.to_string_lossy()))
        }
        "convert_table_to_xmacro" => serde_json::to_value(table_xmacro::convert(
            request.path,
            request.clipboard_text,
            request.macro_name,
            request.struct_name,
        )?)
        .map_err(|error| error.to_string()),
        "read_excel_document" => serde_json::to_value(excel_reader::read_document(&request.path)?)
            .map_err(|error| error.to_string()),
        _ => Err(format!("不支持的操作：{}", request.action)),
    }
}

fn main() {
    let response = (|| {
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| error.to_string())?;
        let request: Request = serde_json::from_str(&input).map_err(|error| error.to_string())?;
        run(request)
    })();
    let response = match response {
        Ok(data) => Response {
            ok: true,
            data,
            error: None,
        },
        Err(error) => Response {
            ok: false,
            data: Value::Null,
            error: Some(error),
        },
    };
    serde_json::to_writer(io::stdout(), &response).expect("无法写出处理结果");
}
