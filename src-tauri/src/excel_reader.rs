use base64::{engine::general_purpose::STANDARD, Engine as _};
use calamine::{open_workbook_auto, Data, Reader};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;

const MAX_ROWS: usize = 50_000;
const MAX_COLUMNS: usize = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelDocument {
    file_name: String,
    source_path: String,
    sheets: Vec<ExcelSheet>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelSheet {
    name: String,
    headers: Vec<String>,
    rows: Vec<ExcelRow>,
    outline: Vec<OutlineItem>,
    images: Vec<CellImage>,
    header_row: usize,
    total_rows: usize,
    total_columns: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelRow {
    row_number: usize,
    cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineItem {
    row_number: usize,
    number: String,
    title: String,
    level: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CellImage {
    row: usize,
    column: usize,
    name: String,
    mime_type: String,
    data_url: String,
}

pub fn read_document(raw_path: &str) -> Result<ExcelDocument, String> {
    let path = Path::new(raw_path.trim());
    if !path.is_file() {
        return Err("请选择有效的 Excel 文件。".to_string());
    }

    let mut workbook = open_workbook_auto(path)
        .map_err(|error| format!("无法打开表格，请确认文件未加密且格式受支持：{error}"))?;
    let pictures = workbook.pictures_with_metadata();
    let mut pictures_by_sheet: HashMap<String, Vec<CellImage>> = HashMap::new();
    let mut warnings = Vec::new();

    for (index, picture) in pictures.into_iter().enumerate() {
        if picture.sheet_name.is_empty() {
            warnings.push(format!("图片 {} 没有工作表定位信息，已跳过。", index + 1));
            continue;
        }
        let extension = picture
            .extension
            .trim_start_matches('.')
            .to_ascii_lowercase();
        let mime_type = image_mime(&extension).to_string();
        let data_url = format!("data:{mime_type};base64,{}", STANDARD.encode(&picture.data));
        pictures_by_sheet
            .entry(picture.sheet_name)
            .or_default()
            .push(CellImage {
                row: picture.row as usize + 1,
                column: picture.col as usize,
                name: if picture.name.trim().is_empty() {
                    format!("图片 {}", index + 1)
                } else {
                    picture.name
                },
                mime_type,
                data_url,
            });
    }

    let sheet_names = workbook.sheet_names().to_vec();
    let mut sheets = Vec::new();
    for sheet_name in sheet_names {
        let images = pictures_by_sheet.remove(&sheet_name).unwrap_or_default();
        let range = match workbook.worksheet_range(&sheet_name) {
            Ok(range) => range,
            Err(error) => {
                warnings.push(format!("工作表“{sheet_name}”读取失败：{error}"));
                continue;
            }
        };
        let (start_row, start_col) = range
            .start()
            .map(|(row, col)| (row as usize, col as usize))
            .unwrap_or((0, 0));
        let source_height = range.height();
        let image_last_row = images.iter().map(|image| image.row).max().unwrap_or(0);
        let total_rows = source_height.saturating_add(start_row).max(image_last_row);
        let image_columns = images
            .iter()
            .map(|image| image.column + 1)
            .max()
            .unwrap_or(0);
        let total_columns = range
            .width()
            .saturating_add(start_col)
            .max(image_columns)
            .min(MAX_COLUMNS);
        let mut rows = Vec::new();

        for (relative_row, values) in range.rows().take(MAX_ROWS).enumerate() {
            let mut cells = vec![String::new(); start_col.min(MAX_COLUMNS)];
            cells.extend(
                values
                    .iter()
                    .take(MAX_COLUMNS.saturating_sub(cells.len()))
                    .map(cell_text),
            );
            cells.resize(total_columns, String::new());
            rows.push(ExcelRow {
                row_number: start_row + relative_row + 1,
                cells,
            });
        }

        if let Some(first_image_row) = images.iter().map(|image| image.row).min() {
            let first_data_row = rows.first().map(|row| row.row_number).unwrap_or(usize::MAX);
            if first_image_row < first_data_row {
                let mut leading = (first_image_row..first_data_row)
                    .take(MAX_ROWS.saturating_sub(rows.len()))
                    .map(|row_number| ExcelRow {
                        row_number,
                        cells: vec![String::new(); total_columns],
                    })
                    .collect::<Vec<_>>();
                leading.append(&mut rows);
                rows = leading;
            }
        }
        let last_data_row = rows.last().map(|row| row.row_number).unwrap_or(0);
        for row_number in (last_data_row + 1)..=total_rows {
            if rows.len() >= MAX_ROWS {
                break;
            }
            rows.push(ExcelRow {
                row_number,
                cells: vec![String::new(); total_columns],
            });
        }

        if source_height > MAX_ROWS {
            warnings.push(format!(
                "工作表“{sheet_name}”有 {source_height} 行，为保证浏览流畅仅显示前 {MAX_ROWS} 行。"
            ));
        }
        if range.width().saturating_add(start_col) > MAX_COLUMNS {
            warnings.push(format!(
                "工作表“{sheet_name}”列数较多，为保证浏览流畅仅显示前 {MAX_COLUMNS} 列。"
            ));
        }

        let header_index = detect_header(&rows);
        let headers = build_headers(&rows, header_index, total_columns);
        let outline = build_outline(&rows, header_index);
        sheets.push(ExcelSheet {
            name: sheet_name,
            headers,
            rows,
            outline,
            images,
            header_row: header_index,
            total_rows,
            total_columns,
        });
    }

    if sheets.is_empty() {
        return Err("工作簿中没有可读取的工作表。".to_string());
    }

    Ok(ExcelDocument {
        file_name: path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("Excel 文档")
            .to_string(),
        source_path: path.to_string_lossy().into_owned(),
        sheets,
        warnings,
    })
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        Data::Float(number) if number.fract() == 0.0 => format!("{number:.0}"),
        _ => value.to_string(),
    }
}

fn detect_header(rows: &[ExcelRow]) -> usize {
    let mut best = (0_usize, 0_usize);
    for (index, row) in rows.iter().take(40).enumerate() {
        let score = row.cells.iter().map(|value| header_score(value)).sum();
        if score > best.1 {
            best = (index, score);
        }
    }
    if best.1 >= 3 {
        best.0
    } else {
        rows.iter()
            .position(|row| row.cells.iter().any(|value| !value.trim().is_empty()))
            .unwrap_or(0)
    }
}

fn header_score(value: &str) -> usize {
    match normalize(value).as_str() {
        "id" | "编号" | "序号" => 2,
        "标题编号" | "章节编号" | "section" | "sectionnumber" | "headingnumber" => 5,
        "需求内容"
        | "需求内容entrycontent"
        | "entrycontent"
        | "requirement"
        | "requirements"
        | "content"
        | "标题"
        | "title" => 5,
        "anchor" | "doorsid" | "controllevel" | "image" | "图片" | "是否是需求" => 2,
        _ => 0,
    }
}

fn build_headers(rows: &[ExcelRow], header_index: usize, columns: usize) -> Vec<String> {
    let source = rows
        .get(header_index)
        .map(|row| row.cells.as_slice())
        .unwrap_or(&[]);
    (0..columns)
        .map(|column| {
            let label = source.get(column).map(|value| value.trim()).unwrap_or("");
            if label.is_empty() {
                column_label(column)
            } else {
                label.to_string()
            }
        })
        .collect()
}

fn build_outline(rows: &[ExcelRow], header_index: usize) -> Vec<OutlineItem> {
    let header = rows
        .get(header_index)
        .map(|row| row.cells.as_slice())
        .unwrap_or(&[]);
    let number_column = header.iter().position(|value| {
        matches!(
            normalize(value).as_str(),
            "标题编号" | "章节编号" | "section" | "sectionnumber" | "headingnumber"
        )
    });
    let content_column = header.iter().position(|value| {
        matches!(
            normalize(value).as_str(),
            "需求内容"
                | "需求内容entrycontent"
                | "entrycontent"
                | "requirement"
                | "requirements"
                | "content"
                | "标题"
                | "title"
        )
    });

    let mut outline = Vec::new();
    for row in rows.iter().skip(header_index.saturating_add(1)) {
        let candidate = number_column
            .and_then(|column| row.cells.get(column))
            .map(|value| value.trim())
            .filter(|value| is_section_number(value));
        let (number, number_index) = if let Some(value) = candidate {
            (value, number_column.unwrap_or(0))
        } else if number_column.is_none() {
            let Some((column, value)) = row
                .cells
                .iter()
                .enumerate()
                .find(|(_, value)| is_section_number(value.trim()))
            else {
                continue;
            };
            (value.trim(), column)
        } else {
            continue;
        };

        let title = content_column
            .and_then(|column| row.cells.get(column))
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                row.cells
                    .iter()
                    .skip(number_index + 1)
                    .find(|value| !value.trim().is_empty())
            })
            .map(|value| one_line(value, 160))
            .unwrap_or_else(|| "未命名章节".to_string());
        outline.push(OutlineItem {
            row_number: row.row_number,
            number: number.trim_end_matches('.').to_string(),
            title,
            level: number.trim_end_matches('.').split('.').count().max(1),
        });
    }
    outline
}

fn is_section_number(value: &str) -> bool {
    let trimmed = value.trim().trim_end_matches('.');
    !trimmed.is_empty()
        && trimmed.len() <= 48
        && trimmed.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

fn one_line(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        compact
    } else {
        format!("{}…", compact.chars().take(max_chars).collect::<String>())
    }
}

fn normalize(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| {
            !character.is_whitespace() && !matches!(character, '_' | '-' | '/' | '\\')
        })
        .collect()
}

fn column_label(mut column: usize) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, (b'A' + (column % 26) as u8) as char);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    label
}

fn image_mime(extension: &str) -> &'static str {
    match extension {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        _ => "image/png",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(extension: &str) -> std::path::PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("struct-sheet-reader-{id}.{extension}"))
    }

    #[test]
    fn reads_headers_and_builds_outline() {
        let path = temp_path("xlsx");
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.write_string(0, 0, "ID").unwrap();
        sheet.write_string(0, 1, "标题编号").unwrap();
        sheet.write_string(0, 2, "需求内容/entrycontent").unwrap();
        sheet.write_number(1, 0, 3).unwrap();
        sheet.write_string(1, 1, "1.").unwrap();
        sheet.write_string(1, 2, "INTRODUCTION").unwrap();
        sheet.write_number(2, 0, 4).unwrap();
        sheet.write_string(2, 1, "1.1").unwrap();
        sheet.write_string(2, 2, "PURPOSE").unwrap();
        sheet.write_number(3, 0, 5).unwrap();
        sheet
            .write_string(3, 1, "1.1.1.1.1.1.1.1.1.1.1.1.1")
            .unwrap();
        sheet.write_string(3, 2, "DEEP SECTION").unwrap();
        workbook.save(&path).unwrap();

        let document = read_document(&path.to_string_lossy()).unwrap();
        assert_eq!(document.sheets[0].headers[1], "标题编号");
        assert_eq!(document.sheets[0].outline.len(), 3);
        assert_eq!(document.sheets[0].outline[1].level, 2);
        assert_eq!(document.sheets[0].outline[1].title, "PURPOSE");
        assert_eq!(document.sheets[0].outline[2].level, 13);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn preserves_embedded_image_position() {
        // A valid 1x1 transparent PNG.
        let png = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=")
            .unwrap();
        let path = temp_path("xlsx");
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.set_name("Requirements").unwrap();
        sheet.write_string(0, 0, "ID").unwrap();
        sheet.write_string(0, 1, "Image").unwrap();
        let image = rust_xlsxwriter::Image::new_from_buffer(&png).unwrap();
        sheet.insert_image(2, 1, &image).unwrap();
        workbook.save(&path).unwrap();

        let document = read_document(&path.to_string_lossy()).unwrap();
        let image = &document.sheets[0].images[0];
        assert_eq!(image.row, 3);
        assert_eq!(image.column, 1);
        assert!(image.data_url.starts_with("data:image/png;base64,"));
        fs::remove_file(path).unwrap();
    }
}
