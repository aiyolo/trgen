use calamine::{open_workbook_auto, Reader};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug)]
struct Request {
    path: String,
    clipboard_text: String,
    macro_name: String,
    struct_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OutputRow {
    source_row: usize,
    byte_spec: String,
    name: String,
    data_type: String,
    offset: u64,
    size: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultData {
    source_path: String,
    sheet_name: String,
    macro_name: String,
    struct_name: String,
    row_count: usize,
    code: String,
    warnings: Vec<String>,
    rows: Vec<OutputRow>,
}

#[derive(Debug, Clone, Copy)]
struct Columns {
    offset: Option<usize>,
    name: usize,
    data_type: usize,
    size: Option<usize>,
}

#[derive(Debug)]
struct TableCandidate {
    sheet_name: String,
    rows: Vec<Vec<String>>,
    header_row: usize,
    columns: Columns,
    score: usize,
}

pub fn convert(
    path: String,
    clipboard_text: String,
    macro_name: String,
    struct_name: String,
) -> Result<ResultData, String> {
    convert_request(Request {
        path,
        clipboard_text,
        macro_name,
        struct_name,
    })
}

fn convert_request(request: Request) -> Result<ResultData, String> {
    let macro_name = c_identifier(&request.macro_name, true, "TABLE_FIELDS");
    let struct_name = c_identifier(&request.struct_name, false, "TABLE_TYPE");
    let (source_path, tables) = if !request.clipboard_text.trim().is_empty() {
        (
            "粘贴的 Excel 内容".to_string(),
            vec![(
                "粘贴内容".to_string(),
                parse_delimited(&request.clipboard_text, '\t'),
            )],
        )
    } else {
        let path = Path::new(request.path.trim());
        if !path.is_file() {
            return Err("请选择表格文件，或粘贴从 Excel/WPS 复制的单元格内容。".to_string());
        }
        (path.to_string_lossy().into_owned(), read_tables(path)?)
    };
    let candidate = choose_table(tables)?;
    let (rows, mut warnings) = parse_rows(&candidate)?;
    if rows.is_empty() {
        return Err("找到了表头，但没有找到可转换的字段行。".to_string());
    }
    if request.macro_name.trim() != macro_name {
        warnings.push(format!("宏名称已规范为 `{macro_name}`。"));
    }
    if request.struct_name.trim() != struct_name {
        warnings.push(format!("结构体名称已规范为 `{struct_name}`。"));
    }
    let code = generate_code(&macro_name, &struct_name, &rows);
    Ok(ResultData {
        source_path,
        sheet_name: candidate.sheet_name,
        macro_name,
        struct_name,
        row_count: rows.len(),
        code,
        warnings,
        rows,
    })
}

fn read_tables(path: &Path) -> Result<Vec<(String, Vec<Vec<String>>)>, String> {
    if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("csv"))
    {
        let content = fs::read_to_string(path)
            .map_err(|error| format!("CSV 读取失败，请确认文件为 UTF-8 编码：{error}"))?;
        return Ok(vec![("CSV".to_string(), parse_delimited(&content, ','))]);
    }

    let mut workbook = open_workbook_auto(path)
        .map_err(|error| format!("无法打开表格，请确认格式受支持且文件未损坏：{error}"))?;
    let names = workbook.sheet_names().to_vec();
    let mut tables = Vec::new();
    for name in names {
        match workbook.worksheet_range(&name) {
            Ok(range) => {
                let rows = range
                    .rows()
                    .map(|row| row.iter().map(ToString::to_string).collect())
                    .collect();
                tables.push((name, rows));
            }
            Err(_) => continue,
        }
    }
    if tables.is_empty() {
        Err("工作簿中没有可读取的工作表。".to_string())
    } else {
        Ok(tables)
    }
}

fn choose_table(tables: Vec<(String, Vec<Vec<String>>)>) -> Result<TableCandidate, String> {
    let mut best: Option<TableCandidate> = None;
    for (sheet_name, rows) in tables {
        for (header_row, row) in rows.iter().take(100).enumerate() {
            let mut offset = None;
            let mut name = None;
            let mut data_type = None;
            let mut size = None;
            for (column, value) in row.iter().enumerate() {
                match header_kind(value) {
                    Some("offset") if offset.is_none() => offset = Some(column),
                    Some("name") if name.is_none() => name = Some(column),
                    Some("type") if data_type.is_none() => data_type = Some(column),
                    Some("size") if size.is_none() => size = Some(column),
                    _ => {}
                }
            }
            let (Some(name), Some(data_type)) = (name, data_type) else {
                continue;
            };
            let score = 2 + usize::from(offset.is_some()) * 2 + usize::from(size.is_some());
            let candidate = TableCandidate {
                sheet_name: sheet_name.clone(),
                rows: rows.clone(),
                header_row,
                columns: Columns {
                    offset,
                    name,
                    data_type,
                    size,
                },
                score,
            };
            if best
                .as_ref()
                .is_none_or(|current| candidate.score > current.score)
            {
                best = Some(candidate);
            }
        }
    }
    best.ok_or_else(|| {
        "未识别到字段表头。至少需要 Parameter Name（字段名）和 Type（类型）两列。".to_string()
    })
}

fn parse_rows(candidate: &TableCandidate) -> Result<(Vec<OutputRow>, Vec<String>), String> {
    let mut output = Vec::new();
    let mut warnings = Vec::new();
    let mut next_offset = 0_u64;
    let mut empty_run = 0_usize;

    for (row_index, row) in candidate
        .rows
        .iter()
        .enumerate()
        .skip(candidate.header_row + 1)
    {
        let raw_name = cell(row, candidate.columns.name);
        let data_type = cell(row, candidate.columns.data_type).trim().to_string();
        let byte_spec = candidate
            .columns
            .offset
            .map(|column| cell(row, column).trim().to_string())
            .unwrap_or_default();
        if raw_name.trim().is_empty() && data_type.is_empty() && byte_spec.is_empty() {
            empty_run += 1;
            if empty_run >= 20 && !output.is_empty() {
                break;
            }
            continue;
        }
        empty_run = 0;
        if raw_name.trim().is_empty() || data_type.is_empty() {
            warnings.push(format!("第 {} 行缺少字段名或类型，已跳过。", row_index + 1));
            continue;
        }
        let name = c_identifier(raw_name.trim(), false, "field");
        if name != raw_name.trim() {
            warnings.push(format!(
                "第 {} 行字段名 `{}` 已规范为 `{name}`。",
                row_index + 1,
                raw_name.trim()
            ));
        }
        let range = parse_byte_range(&byte_spec);
        let explicit_size = candidate
            .columns
            .size
            .and_then(|column| first_u64(cell(row, column)));
        let type_size = type_size(&data_type);
        let offset = range.map(|value| value.0).unwrap_or(next_offset);
        let size = range
            .map(|value| value.1)
            .filter(|value| *value > 0)
            .or(explicit_size)
            .or(type_size)
            .unwrap_or_else(|| {
                warnings.push(format!(
                    "第 {} 行类型 `{data_type}` 无法推断大小，按 1 字节生成。",
                    row_index + 1
                ));
                1
            });
        next_offset = offset.saturating_add(size);
        output.push(OutputRow {
            source_row: row_index + 1,
            byte_spec,
            name,
            data_type,
            offset,
            size,
        });
    }
    Ok((output, warnings))
}

fn generate_code(macro_name: &str, struct_name: &str, rows: &[OutputRow]) -> String {
    let mut output = format!("#define {macro_name}(X) \\\n");
    for (index, row) in rows.iter().enumerate() {
        output.push_str(&format!(
            "    X({}, {}, {}, {})",
            row.data_type, row.name, row.offset, row.size
        ));
        if index + 1 < rows.len() {
            output.push_str(" \\\n");
        } else {
            output.push('\n');
        }
    }
    output.push_str(&format!(
        "\ntypedef struct\n{{\n    {macro_name}(DECLARE_FIELD)\n}} {struct_name};\n"
    ));
    output
}

fn header_kind(value: &str) -> Option<&'static str> {
    let normalized: String = value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|character| !character.is_whitespace() && !matches!(character, '_' | '-'))
        .collect();
    match normalized.as_str() {
        "bytes" | "byte" | "offset" | "iobufferoffset" | "字节" | "偏移" | "地址" => {
            Some("offset")
        }
        "parametername" | "fieldname" | "name" | "参数名称" | "字段名" | "变量名" => {
            Some("name")
        }
        "type" | "datatype" | "类型" | "数据类型" => Some("type"),
        "size" | "datasize" | "length" | "大小" | "长度" => Some("size"),
        _ => None,
    }
}

fn parse_byte_range(value: &str) -> Option<(u64, u64)> {
    if let Ok(number) = value.trim().parse::<f64>() {
        if number.is_finite() && number >= 0.0 && number.fract() == 0.0 {
            return Some((number as u64, 0));
        }
    }
    let numbers: Vec<u64> = value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect();
    match numbers.as_slice() {
        [start, end, ..] if end >= start => Some((*start, end - start + 1)),
        [offset] => Some((*offset, 0)),
        _ => None,
    }
}

fn first_u64(value: &str) -> Option<u64> {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .and_then(|number| (number.is_finite() && number >= 0.0).then_some(number as u64))
}

fn type_size(data_type: &str) -> Option<u64> {
    let compact = data_type
        .to_lowercase()
        .replace("const", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let array_multiplier = array_multiplier(&compact);
    let base = compact.split('[').next().unwrap_or(&compact).trim();
    let size = if base.contains('*') {
        8
    } else {
        match base {
            "char" | "signed char" | "unsigned char" | "int8_t" | "uint8_t" | "bool" | "_bool" => 1,
            "short" | "short int" | "signed short" | "signed short int" | "unsigned short"
            | "unsigned short int" | "int16_t" | "uint16_t" => 2,
            "int" | "signed" | "signed int" | "unsigned" | "unsigned int" | "long" | "long int"
            | "unsigned long" | "unsigned long int" | "float" | "int32_t" | "uint32_t" => 4,
            "long long"
            | "long long int"
            | "unsigned long long"
            | "unsigned long long int"
            | "double"
            | "int64_t"
            | "uint64_t"
            | "size_t" => 8,
            "long double" => 16,
            _ => return None,
        }
    };
    Some(size * array_multiplier)
}

fn array_multiplier(value: &str) -> u64 {
    let mut multiplier = 1_u64;
    let mut rest = value;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            break;
        };
        if let Ok(length) = after[..close].trim().parse::<u64>() {
            multiplier = multiplier.saturating_mul(length.max(1));
        }
        rest = &after[close + 1..];
    }
    multiplier
}

fn c_identifier(value: &str, uppercase: bool, fallback: &str) -> String {
    let value = value.trim();
    let mut output = String::new();
    for character in value.chars() {
        let character = if character.is_ascii_alphanumeric() || character == '_' {
            character
        } else {
            '_'
        };
        output.push(if uppercase {
            character.to_ascii_uppercase()
        } else {
            character
        });
    }
    while output.contains("__") {
        output = output.replace("__", "_");
    }
    output = output.trim_matches('_').to_string();
    if output.is_empty() {
        output = fallback.to_string();
    }
    if output.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        output.insert(0, '_');
    }
    output
}

fn parse_delimited(content: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut characters = content.trim_start_matches('\u{feff}').chars().peekable();
    let mut quoted = false;
    while let Some(character) = characters.next() {
        match character {
            '"' if quoted && characters.peek() == Some(&'"') => {
                field.push('"');
                characters.next();
            }
            '"' => quoted = !quoted,
            value if value == delimiter && !quoted => row.push(std::mem::take(&mut field)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            '\r' if !quoted => {}
            _ => field.push(character),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

fn cell(row: &[String], column: usize) -> &str {
    row.get(column).map(String::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_byte_ranges_and_generates_expected_macro() {
        let candidate = TableCandidate {
            sheet_name: "Sheet1".to_string(),
            rows: vec![
                vec!["Bytes".into(), "Parameter Name".into(), "Type".into()],
                vec!["0~3".into(), "IP_GPM_value".into(), "float".into()],
                vec!["5".into(), "IP_GPM_status".into(), "unsigned char".into()],
            ],
            header_row: 0,
            columns: Columns {
                offset: Some(0),
                name: 1,
                data_type: 2,
                size: None,
            },
            score: 4,
        };
        let (rows, warnings) = parse_rows(&candidate).unwrap();
        assert!(warnings.is_empty());
        assert_eq!((rows[0].offset, rows[0].size), (0, 4));
        // A single byte number is an offset; its size comes from the C type.
        assert_eq!((rows[1].offset, rows[1].size), (5, 1));
        let code = generate_code("EI_FIELDS", "EI_TYPE", &rows);
        assert!(code.contains("X(float, IP_GPM_value, 0, 4)"));
        assert!(code.contains("X(unsigned char, IP_GPM_status, 5, 1)"));
        assert!(code.contains("EI_FIELDS(DECLARE_FIELD)"));
    }

    #[test]
    fn converts_a_csv_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("table-xmacro-{unique}.csv"));
        fs::write(
            &path,
            "Bytes,Parameter Name,Type\n0~3,value,float\n4,status,unsigned char\n",
        )
        .unwrap();
        let result = convert_request(Request {
            path: path.to_string_lossy().into_owned(),
            clipboard_text: String::new(),
            macro_name: "TEST_FIELDS".into(),
            struct_name: "TEST_TYPE".into(),
        })
        .unwrap();
        assert_eq!(result.row_count, 2);
        assert_eq!(result.rows[1].size, 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn converts_an_xlsx_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("table-xmacro-{unique}.xlsx"));
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let sheet = workbook.add_worksheet();
        sheet.write_string(0, 0, "Bytes").unwrap();
        sheet.write_string(0, 1, "Parameter Name").unwrap();
        sheet.write_string(0, 2, "Type").unwrap();
        sheet.write_string(1, 0, "0~3").unwrap();
        sheet.write_string(1, 1, "airspeed").unwrap();
        sheet.write_string(1, 2, "float").unwrap();
        workbook.save(&path).unwrap();

        let result = convert_request(Request {
            path: path.to_string_lossy().into_owned(),
            clipboard_text: String::new(),
            macro_name: "AIR_FIELDS".into(),
            struct_name: "AIR_TYPE".into(),
        })
        .unwrap();
        assert_eq!(result.sheet_name, "Sheet1");
        assert_eq!((result.rows[0].offset, result.rows[0].size), (0, 4));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn converts_excel_clipboard_text() {
        let result = convert_request(Request {
            path: String::new(),
            clipboard_text: concat!(
                "Bytes\tParameter Name\tType\r\n",
                "0~3\tIP_GPM_airspeed\tfloat\r\n",
                "5\tIP_GPM_status\tunsigned char\r\n"
            )
            .to_string(),
            macro_name: "PFD_FIELDS".into(),
            struct_name: "PFD_TYPE".into(),
        })
        .unwrap();
        assert_eq!(result.sheet_name, "粘贴内容");
        assert_eq!(result.row_count, 2);
        assert!(result
            .code
            .contains("X(unsigned char, IP_GPM_status, 5, 1)"));
    }
}
