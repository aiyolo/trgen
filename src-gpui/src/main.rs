#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../../src-tauri/src/excel.rs"]
mod excel;
#[path = "../../src-tauri/src/excel_reader.rs"]
mod excel_reader;
#[path = "../../src-tauri/src/parser.rs"]
mod parser;
#[path = "../../src-tauri/src/table_xmacro.rs"]
mod table_xmacro;

mod ui;

fn main() {
    ui::run();
}
