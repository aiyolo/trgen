use crate::parser::ParseResult;
use rust_xlsxwriter::{
    Color, Format, FormatAlign, FormatBorder, FormatPattern, Workbook, XlsxError,
};
use std::collections::HashMap;
use std::path::Path;

struct RowFormats {
    text: Format,
    centered: Format,
    comment: Format,
}

pub fn write_workbook(path: &Path, result: &ParseResult) -> Result<(), String> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let sheet_name = safe_sheet_name(&result.root_name);
    worksheet.set_name(&sheet_name).map_err(format_xlsx_error)?;

    let header_format = Format::new()
        .set_bold()
        .set_font_color(Color::RGB(0x2D2A21))
        .set_background_color(Color::RGB(0xFFF200))
        .set_pattern(FormatPattern::Solid)
        .set_border(FormatBorder::Thin)
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);
    // Low-saturation colors keep structure groups easy to scan without competing
    // with the yellow table header.
    let palette = [0xEAF2FF, 0xEAF7F0, 0xFFF1E6, 0xF2EDFF, 0xFFF8D9, 0xE8F6F7];
    let row_formats: Vec<RowFormats> = palette.into_iter().map(make_row_formats).collect();
    let mut group_colors: HashMap<&str, usize> = HashMap::new();
    let mut next_color = 0_usize;

    let headers = [
        "Status",
        "Parameter Name",
        "System Parameter Name",
        "Data Type",
        "Data Size",
        "InIOBuffer",
        "IOBufferOffset",
        "Comments",
    ];
    for (column, header) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(0, column as u16, *header, &header_format)
            .map_err(format_xlsx_error)?;
    }

    for (index, row) in result.rows.iter().enumerate() {
        let excel_row = (index + 1) as u32;
        let color_index = if let Some(index) = group_colors.get(row.group_key.as_str()) {
            *index
        } else {
            let index = next_color % row_formats.len();
            group_colors.insert(row.group_key.as_str(), index);
            next_color += 1;
            index
        };
        let formats = &row_formats[color_index];
        worksheet
            .write_string_with_format(excel_row, 0, &row.status, &formats.centered)
            .and_then(|sheet| {
                sheet.write_string_with_format(excel_row, 1, &row.parameter_name, &formats.text)
            })
            .and_then(|sheet| {
                sheet.write_string_with_format(
                    excel_row,
                    2,
                    &row.system_parameter_name,
                    &formats.text,
                )
            })
            .and_then(|sheet| {
                sheet.write_string_with_format(excel_row, 3, &row.data_type, &formats.centered)
            })
            .and_then(|sheet| {
                sheet.write_number_with_format(
                    excel_row,
                    4,
                    row.data_size as f64,
                    &formats.centered,
                )
            })
            .and_then(|sheet| {
                sheet.write_string_with_format(excel_row, 5, &row.in_io_buffer, &formats.centered)
            })
            .and_then(|sheet| {
                sheet.write_number_with_format(
                    excel_row,
                    6,
                    row.io_buffer_offset as f64,
                    &formats.centered,
                )
            })
            .and_then(|sheet| {
                sheet.write_string_with_format(excel_row, 7, &row.comments, &formats.comment)
            })
            .map_err(format_xlsx_error)?;
    }

    let widths = [11.0, 34.0, 38.0, 14.0, 12.0, 13.0, 16.0, 30.0];
    for (column, width) in widths.iter().enumerate() {
        worksheet
            .set_column_width(column as u16, *width)
            .map_err(format_xlsx_error)?;
    }
    worksheet
        .set_row_height(0, 24.0)
        .map_err(format_xlsx_error)?;
    worksheet
        .set_freeze_panes(1, 0)
        .map_err(format_xlsx_error)?;
    if !result.rows.is_empty() {
        worksheet
            .autofilter(0, 0, result.rows.len() as u32, 7)
            .map_err(format_xlsx_error)?;
    }

    workbook.save(path).map_err(format_xlsx_error)
}

fn make_row_formats(background: u32) -> RowFormats {
    let text = Format::new()
        .set_background_color(Color::RGB(background))
        .set_pattern(FormatPattern::Solid)
        .set_border(FormatBorder::Thin)
        .set_align(FormatAlign::VerticalCenter);
    let centered = Format::new()
        .set_background_color(Color::RGB(background))
        .set_pattern(FormatPattern::Solid)
        .set_border(FormatBorder::Thin)
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);
    let comment = Format::new()
        .set_background_color(Color::RGB(background))
        .set_pattern(FormatPattern::Solid)
        .set_border(FormatBorder::Thin)
        .set_text_wrap()
        .set_align(FormatAlign::VerticalCenter);
    RowFormats {
        text,
        centered,
        comment,
    }
}

fn safe_sheet_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|ch| !matches!(ch, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    if cleaned.is_empty() {
        "Struct Fields".to_string()
    } else {
        cleaned
    }
}

fn format_xlsx_error(error: XlsxError) -> String {
    format!("Excel 文件写入失败：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn creates_a_valid_xlsx_container() {
        let result = parser::parse(
            "typedef struct { uint32_t id; float value; } Packet;",
            Some("Packet"),
        )
        .unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("struct-sheet-{unique}.xlsx"));

        write_workbook(&path, &result).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"PK"));
        assert!(bytes.len() > 1_000);
        fs::remove_file(path).unwrap();
    }
}
