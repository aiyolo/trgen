#![cfg_attr(all(target_os = "windows", not(test)), windows_subsystem = "windows")]

mod excel;
mod excel_reader;
mod parser;
mod table_xmacro;

mod ui;

fn main() {
    ui::run();
}
