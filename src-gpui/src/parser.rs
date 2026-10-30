use serde::Serialize;
use std::collections::{HashMap, HashSet};

const MAX_EXPANDED_ROWS: usize = 10_000;
const MAX_MACRO_EXPANSION_TOKENS: usize = 200_000;
const MAX_MACRO_EXPANSION_DEPTH: usize = 64;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldRow {
    pub status: String,
    pub parameter_name: String,
    pub system_parameter_name: String,
    pub data_type: String,
    pub data_size: u64,
    pub in_io_buffer: String,
    pub io_buffer_offset: u64,
    pub comments: String,
    #[serde(skip_serializing)]
    pub group_key: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub root_name: String,
    pub available_structs: Vec<String>,
    pub total_size: u64,
    pub rows: Vec<FieldRow>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructCatalog {
    pub available_structs: Vec<String>,
    pub default_root: String,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Ident(String),
    Number(u64),
    Symbol(char),
    Comment(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Token {
    kind: TokenKind,
    line: usize,
}

impl Token {
    fn ident(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Ident(value) => Some(value),
            _ => None,
        }
    }

    fn is_ident(&self, expected: &str) -> bool {
        self.ident() == Some(expected)
    }

    fn is_symbol(&self, expected: char) -> bool {
        matches!(self.kind, TokenKind::Symbol(value) if value == expected)
    }
}

#[derive(Debug, Clone)]
struct StructDraft {
    name: String,
    tag: Option<String>,
    body: Vec<Token>,
    pack: Option<u64>,
    is_union: bool,
}

#[derive(Debug, Clone)]
struct StructDef {
    fields: Vec<FieldDef>,
    pack: Option<u64>,
    is_union: bool,
}

#[derive(Debug, Clone)]
struct FieldDef {
    name: String,
    type_name: String,
    pointer_depth: usize,
    array_len: u64,
    bit_width: Option<u64>,
    comment: String,
}

#[derive(Debug, Clone)]
struct TypeLayout {
    size: u64,
    align: u64,
    display_name: String,
    nested_name: Option<String>,
}

#[derive(Debug, Clone)]
struct FieldPlacement {
    offset: u64,
    layout: TypeLayout,
}

#[derive(Debug, Clone)]
struct TypeAlias {
    target: String,
    pointer_depth: usize,
}

#[derive(Debug, Clone)]
struct MacroDefinition {
    params: Option<Vec<String>>,
    replacement: Vec<Token>,
}

pub fn discover(source: &str) -> Result<StructCatalog, String> {
    if source.trim().is_empty() {
        return Err("请输入 C 头文件或结构体定义。".to_string());
    }
    let normalized_source = filter_conditional_blocks(&join_line_continuations(source))?;
    let macros = collect_macro_definitions(&normalized_source)?;
    let tokens = expand_macro_tokens(&tokenize(&normalized_source)?, &macros)?;
    let pack_by_line = collect_pack_by_line(&normalized_source);
    let drafts = collect_structs(&tokens, &pack_by_line)?;
    if drafts.is_empty() {
        return Err("没有找到完整的结构体定义，请确认头文件中包含 `struct { ... }`。".to_string());
    }
    let available_structs: Vec<String> = drafts
        .iter()
        .filter(|item| !item.is_union)
        .map(|item| item.name.clone())
        .collect();
    if available_structs.is_empty() {
        return Err("没有找到完整的结构体定义。".to_string());
    }
    let default_root = available_structs
        .last()
        .cloned()
        .expect("catalog is non-empty");
    Ok(StructCatalog {
        available_structs,
        default_root,
    })
}

pub fn parse(source: &str, requested_root: Option<&str>) -> Result<ParseResult, String> {
    if source.trim().is_empty() {
        return Err("请输入 C 结构体定义。".to_string());
    }

    let normalized_source = filter_conditional_blocks(&join_line_continuations(source))?;
    let macros = collect_macro_definitions(&normalized_source)?;
    let tokens = expand_macro_tokens(&tokenize(&normalized_source)?, &macros)?;
    let pack_by_line = collect_pack_by_line(&normalized_source);
    let drafts = collect_structs(&tokens, &pack_by_line)?;
    if drafts.is_empty() {
        return Err("没有找到结构体定义，请确认代码包含 `struct { ... }`。".to_string());
    }

    let mut aliases = HashMap::new();
    for draft in &drafts {
        aliases.insert(draft.name.clone(), draft.name.clone());
        if let Some(tag) = &draft.tag {
            aliases.insert(tag.clone(), draft.name.clone());
        }
    }

    let type_aliases = collect_type_aliases(&tokens, &aliases);
    let macro_values = collect_integer_macros(&normalized_source);
    let known_structs: HashSet<String> = aliases.keys().cloned().collect();
    let mut definitions = HashMap::new();
    for draft in &drafts {
        let fields = parse_fields(
            &draft.body,
            &known_structs,
            &aliases,
            &type_aliases,
            &macro_values,
        );
        definitions.insert(
            draft.name.clone(),
            StructDef {
                fields,
                pack: draft.pack,
                is_union: draft.is_union,
            },
        );
    }

    let available_structs: Vec<String> = drafts
        .iter()
        .filter(|item| !item.is_union)
        .map(|item| item.name.clone())
        .collect();
    if available_structs.is_empty() {
        return Err("没有找到完整的结构体定义。".to_string());
    }
    let root_name = match requested_root
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        Some(name) => aliases
            .get(name)
            .cloned()
            .ok_or_else(|| format!("未找到根结构体 `{name}`。"))?,
        None => available_structs
            .last()
            .cloned()
            .expect("available structs is non-empty"),
    };

    let mut warnings = Vec::new();
    let mut layout_cache = HashMap::new();
    let mut visiting = HashSet::new();
    let root_layout = struct_layout(
        &root_name,
        &definitions,
        &aliases,
        &mut layout_cache,
        &mut visiting,
        &mut warnings,
    )?;

    let mut rows = Vec::new();
    flatten_struct(
        &root_name,
        "",
        0,
        &definitions,
        &aliases,
        &mut layout_cache,
        &mut warnings,
        &mut rows,
    )?;

    if rows.is_empty() {
        return Err(format!("结构体 `{root_name}` 中没有找到可导出的字段。"));
    }

    warnings.sort();
    warnings.dedup();

    Ok(ParseResult {
        root_name,
        available_structs,
        total_size: root_layout.size,
        rows,
        warnings,
    })
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;

    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
            }
            index += 1;
            continue;
        }

        if ch == '#' {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }

        if ch == '/' && index + 1 < chars.len() && chars[index + 1] == '/' {
            index += 2;
            let start = index;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            let comment: String = chars[start..index].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Comment(comment.trim().to_string()),
                line,
            });
            continue;
        }

        if ch == '/' && index + 1 < chars.len() && chars[index + 1] == '*' {
            index += 2;
            let start = index;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                if chars[index] == '\n' {
                    line += 1;
                }
                index += 1;
            }
            if index + 1 >= chars.len() {
                return Err("存在未闭合的块注释。".to_string());
            }
            let comment: String = chars[start..index].iter().collect();
            tokens.push(Token {
                kind: TokenKind::Comment(
                    comment
                        .lines()
                        .map(|line| line.trim().trim_start_matches('*').trim())
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                ),
                line,
            });
            index += 2;
            continue;
        }

        if ch == '"' || ch == '\'' {
            let quote = ch;
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    index += 2;
                } else if chars[index] == quote {
                    index += 1;
                    break;
                } else {
                    if chars[index] == '\n' {
                        line += 1;
                    }
                    index += 1;
                }
            }
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
            {
                index += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident(chars[start..index].iter().collect()),
                line,
            });
            continue;
        }

        if ch.is_ascii_digit() {
            let start = index;
            index += 1;
            while index < chars.len()
                && (chars[index].is_ascii_hexdigit()
                    || matches!(chars[index], 'x' | 'X' | 'u' | 'U' | 'l' | 'L'))
            {
                index += 1;
            }
            let raw: String = chars[start..index].iter().collect();
            let trimmed = raw.trim_end_matches(['u', 'U', 'l', 'L']);
            let value = parse_c_integer(trimmed).unwrap_or(0);
            tokens.push(Token {
                kind: TokenKind::Number(value),
                line,
            });
            continue;
        }

        tokens.push(Token {
            kind: TokenKind::Symbol(ch),
            line,
        });
        index += 1;
    }

    Ok(tokens)
}

fn parse_c_integer(raw: &str) -> Option<u64> {
    let value = raw.trim().trim_matches(|ch| matches!(ch, '(' | ')'));
    let value = value.trim_end_matches(['u', 'U', 'l', 'L']);
    if value.starts_with("0x") || value.starts_with("0X") {
        u64::from_str_radix(&value[2..], 16).ok()
    } else if value.starts_with("0b") || value.starts_with("0B") {
        u64::from_str_radix(&value[2..], 2).ok()
    } else if value.len() > 1 && value.starts_with('0') {
        u64::from_str_radix(&value[1..], 8)
            .ok()
            .or_else(|| value.parse().ok())
    } else {
        value.parse().ok()
    }
}

fn is_attribute_word(value: &str) -> bool {
    matches!(
        value,
        "__attribute__" | "__declspec" | "alignas" | "_Alignas" | "packed" | "aligned" | "PACKED"
    ) || value.starts_with("__")
}

fn remove_attributes(tokens: &[Token]) -> Vec<Token> {
    let mut cleaned = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let is_attribute = tokens[index]
            .ident()
            .is_some_and(|value| is_attribute_word(value));
        if is_attribute {
            index += 1;
            while index < tokens.len() && matches!(tokens[index].kind, TokenKind::Comment(_)) {
                index += 1;
            }
            if index < tokens.len() && tokens[index].is_symbol('(') {
                let mut depth = 0_i32;
                while index < tokens.len() {
                    if tokens[index].is_symbol('(') {
                        depth += 1;
                    } else if tokens[index].is_symbol(')') {
                        depth -= 1;
                    }
                    index += 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            continue;
        }
        cleaned.push(tokens[index].clone());
        index += 1;
    }
    cleaned
}

fn collect_pack_by_line(source: &str) -> Vec<Option<u64>> {
    let mut values = vec![None; source.lines().count() + 2];
    let mut current = None;
    let mut stack: Vec<Option<u64>> = Vec::new();

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim_start();
        let directive = trimmed
            .strip_prefix('#')
            .map(str::trim_start)
            .and_then(|value| value.strip_prefix("pragma"))
            .map(str::trim_start)
            .and_then(|value| value.strip_prefix("pack"))
            .map(str::trim_start);

        if let Some(directive) = directive {
            if let Some(inner) = directive
                .strip_prefix('(')
                .and_then(|value| value.split(')').next())
            {
                let parts: Vec<&str> = inner
                    .split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .collect();
                match parts.first().copied() {
                    Some("push") => {
                        stack.push(current);
                        if let Some(pack) =
                            parts.iter().rev().find_map(|part| parse_c_integer(part))
                        {
                            if pack > 0 {
                                current = Some(pack);
                            }
                        }
                    }
                    Some("pop") => {
                        current = stack.pop().unwrap_or(None);
                    }
                    Some(value) => {
                        current = parse_c_integer(value).filter(|pack| *pack > 0);
                    }
                    None => current = None,
                }
            }
        }
        values[line_number] = current;
    }
    values
}

fn collect_structs(
    tokens: &[Token],
    pack_by_line: &[Option<u64>],
) -> Result<Vec<StructDraft>, String> {
    let mut drafts = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let typedef = tokens[index].is_ident("typedef");
        let struct_index = if typedef {
            let mut candidate = index + 1;
            let mut found = None;
            while candidate < tokens.len() && !tokens[candidate].is_symbol(';') {
                if tokens[candidate].is_ident("struct") || tokens[candidate].is_ident("union") {
                    found = Some(candidate);
                    break;
                }
                candidate += 1;
            }
            found
        } else {
            Some(index)
        };

        let Some(struct_index) = struct_index else {
            index += 1;
            continue;
        };
        if !tokens[struct_index].is_ident("struct") && !tokens[struct_index].is_ident("union") {
            index += 1;
            continue;
        }
        let is_union = tokens[struct_index].is_ident("union");

        let mut cursor = struct_index + 1;
        let mut open = None;
        let mut paren_depth = 0_i32;
        while cursor < tokens.len() {
            if tokens[cursor].is_symbol(';') && paren_depth == 0 {
                break;
            }
            if tokens[cursor].is_symbol('(') {
                paren_depth += 1;
            } else if tokens[cursor].is_symbol(')') {
                paren_depth -= 1;
            } else if tokens[cursor].is_symbol('{') && paren_depth == 0 {
                open = Some(cursor);
                break;
            }
            cursor += 1;
        }
        let Some(open) = open else {
            index += 1;
            continue;
        };

        let prefix = remove_attributes(&tokens[struct_index + 1..open]);
        let tag = prefix
            .iter()
            .filter_map(Token::ident)
            .filter(|name| !is_attribute_word(name))
            .next_back()
            .map(str::to_string);

        let close = matching_brace(tokens, open)?;
        let mut declaration_end = close + 1;
        while declaration_end < tokens.len() && !tokens[declaration_end].is_symbol(';') {
            declaration_end += 1;
        }
        let has_packed_attribute = tokens[struct_index..open]
            .iter()
            .chain(tokens[close + 1..declaration_end].iter())
            .any(|token| token.ident() == Some("packed") || token.ident() == Some("PACKED"));
        let pack = if has_packed_attribute {
            Some(1)
        } else {
            pack_by_line.get(tokens[open].line).copied().flatten()
        };
        let (name, end_index) = if typedef {
            let after = declaration_end;
            let suffix = remove_attributes(&tokens[close + 1..after]);
            let first_alias = suffix
                .split(|token| token.is_symbol(','))
                .next()
                .unwrap_or(&[]);
            let alias = first_alias
                .iter()
                .filter_map(Token::ident)
                .find(|name| !is_attribute_word(name))
                .map(str::to_string);
            (
                alias
                    .or_else(|| tag.clone())
                    .ok_or_else(|| "typedef 聚合类型缺少名称。".to_string())?,
                after,
            )
        } else {
            (
                tag.clone()
                    .ok_or_else(|| "匿名 struct 需要通过 typedef 指定名称。".to_string())?,
                close,
            )
        };

        if !drafts.iter().any(|item: &StructDraft| item.name == name) {
            drafts.push(StructDraft {
                name,
                tag,
                body: tokens[open + 1..close].to_vec(),
                pack,
                is_union,
            });
        }
        index = end_index.saturating_add(1);
    }

    Ok(drafts)
}

fn collect_type_aliases(
    tokens: &[Token],
    struct_aliases: &HashMap<String, String>,
) -> HashMap<String, TypeAlias> {
    let mut result = HashMap::new();
    let mut index = 0;
    while index < tokens.len() {
        if !tokens[index].is_ident("typedef") {
            index += 1;
            continue;
        }

        let start = index + 1;
        let mut end = start;
        let mut brace_depth = 0_i32;
        while end < tokens.len() {
            if tokens[end].is_symbol('{') {
                brace_depth += 1;
            } else if tokens[end].is_symbol('}') {
                brace_depth -= 1;
            } else if tokens[end].is_symbol(';') && brace_depth == 0 {
                break;
            }
            end += 1;
        }
        let clean = remove_attributes(&tokens[start..end]);
        if clean.iter().any(|token| token.is_symbol('{')) {
            index = end.saturating_add(1);
            continue;
        }
        let first = clean
            .split(|token| token.is_symbol(','))
            .next()
            .unwrap_or(&[]);
        let Some(alias_index) = declarator_name_index(first) else {
            index = end.saturating_add(1);
            continue;
        };
        let Some(alias_name) = first[alias_index].ident() else {
            index = end.saturating_add(1);
            continue;
        };
        let pointer_depth = first[..alias_index]
            .iter()
            .filter(|token| token.is_symbol('*'))
            .count();
        let words: Vec<&str> = first[..alias_index]
            .iter()
            .filter_map(Token::ident)
            .filter(|word| !is_qualifier(word) && !is_attribute_word(word))
            .collect();
        let target = if words.first() == Some(&"struct") || words.first() == Some(&"union") {
            words
                .get(1)
                .and_then(|name| struct_aliases.get(*name).cloned())
                .or_else(|| words.get(1).map(|name| (*name).to_string()))
                .unwrap_or_default()
        } else if words.first() == Some(&"enum") {
            "enum".to_string()
        } else {
            words.join(" ")
        };
        if !target.is_empty() && target != alias_name {
            result.insert(
                alias_name.to_string(),
                TypeAlias {
                    target,
                    pointer_depth,
                },
            );
        }
        index = end.saturating_add(1);
    }
    result
}

fn join_line_continuations(source: &str) -> String {
    source.replace("\\\r\n", " ").replace("\\\n", " ")
}

#[derive(Debug, Clone, Copy)]
struct ConditionalFrame {
    parent_active: bool,
    branch_taken: bool,
}

fn filter_conditional_blocks(source: &str) -> Result<String, String> {
    let mut output = String::with_capacity(source.len());
    let mut frames: Vec<ConditionalFrame> = Vec::new();
    let mut active = true;
    let mut defined: HashSet<String> = ["_WIN32", "_WIN64"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let mut object_macros: HashMap<String, String> = [
        ("_WIN32".to_string(), "1".to_string()),
        ("_WIN64".to_string(), "1".to_string()),
    ]
    .into_iter()
    .collect();

    for (line_index, line) in source.lines().enumerate() {
        let directive = line
            .trim_start()
            .strip_prefix('#')
            .map(str::trim_start)
            .map(|value| {
                let keyword_end = value.find(char::is_whitespace).unwrap_or(value.len());
                (&value[..keyword_end], value[keyword_end..].trim_start())
            });
        let mut keep_line = active;

        match directive {
            Some(("if", expression)) => {
                let condition =
                    active && evaluate_preprocessor_condition(expression, &defined, &object_macros);
                frames.push(ConditionalFrame {
                    parent_active: active,
                    branch_taken: condition,
                });
                active = condition;
                keep_line = false;
            }
            Some(("ifdef", name)) => {
                let condition = active && defined.contains(first_identifier(name).unwrap_or(""));
                frames.push(ConditionalFrame {
                    parent_active: active,
                    branch_taken: condition,
                });
                active = condition;
                keep_line = false;
            }
            Some(("ifndef", name)) => {
                let condition = active && !defined.contains(first_identifier(name).unwrap_or(""));
                frames.push(ConditionalFrame {
                    parent_active: active,
                    branch_taken: condition,
                });
                active = condition;
                keep_line = false;
            }
            Some(("elif", expression)) => {
                let Some(frame) = frames.last_mut() else {
                    return Err(format!("第 {} 行出现了多余的 #elif。", line_index + 1));
                };
                let condition = frame.parent_active
                    && !frame.branch_taken
                    && evaluate_preprocessor_condition(expression, &defined, &object_macros);
                frame.branch_taken |= condition;
                active = condition;
                keep_line = false;
            }
            Some(("else", _)) => {
                let Some(frame) = frames.last_mut() else {
                    return Err(format!("第 {} 行出现了多余的 #else。", line_index + 1));
                };
                active = frame.parent_active && !frame.branch_taken;
                frame.branch_taken = true;
                keep_line = false;
            }
            Some(("endif", _)) => {
                let Some(frame) = frames.pop() else {
                    return Err(format!("第 {} 行出现了多余的 #endif。", line_index + 1));
                };
                active = frame.parent_active;
                keep_line = false;
            }
            Some(("define", definition)) if active => {
                if let Some((name, function_like, replacement)) = macro_signature(definition) {
                    defined.insert(name.clone());
                    if function_like {
                        object_macros.remove(&name);
                    } else {
                        object_macros.insert(name, replacement.to_string());
                    }
                }
            }
            Some(("undef", name)) if active => {
                if let Some(name) = first_identifier(name) {
                    defined.remove(name);
                    object_macros.remove(name);
                }
                keep_line = false;
            }
            Some(_) => {}
            None => {}
        }

        if keep_line {
            output.push_str(line);
        }
        output.push('\n');
    }

    if !frames.is_empty() {
        return Err("存在未闭合的条件编译指令，请检查 #if/#ifdef 与 #endif。".to_string());
    }
    Ok(output)
}

fn first_identifier(value: &str) -> Option<&str> {
    let length = value
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .count();
    (length > 0).then_some(&value[..length])
}

fn macro_signature(definition: &str) -> Option<(String, bool, &str)> {
    let name = first_identifier(definition)?;
    let suffix = &definition[name.len()..];
    if suffix.starts_with('(') {
        let close = matching_macro_parenthesis(suffix)?;
        Some((name.to_string(), true, suffix[close + 1..].trim()))
    } else {
        Some((name.to_string(), false, suffix.trim()))
    }
}

fn evaluate_preprocessor_condition(
    expression: &str,
    defined: &HashSet<String>,
    object_macros: &HashMap<String, String>,
) -> bool {
    let mut values = HashMap::new();
    for _ in 0..32 {
        let mut changed = false;
        for (name, replacement) in object_macros {
            if values.contains_key(name) {
                continue;
            }
            if let Ok(tokens) = tokenize(replacement) {
                if let Some(value) = evaluate_constant(&tokens, &values) {
                    values.insert(name.clone(), value);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    let Ok(tokens) = tokenize(expression) else {
        return false;
    };
    let mut normalized = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].is_ident("defined") {
            let parenthesized = tokens
                .get(index + 1)
                .is_some_and(|token| token.is_symbol('('));
            let name_index = index + if parenthesized { 2 } else { 1 };
            let Some(name) = tokens.get(name_index).and_then(Token::ident) else {
                return false;
            };
            normalized.push(Token {
                kind: TokenKind::Number(u64::from(defined.contains(name))),
                line: tokens[index].line,
            });
            index = name_index + 1;
            if parenthesized && tokens.get(index).is_some_and(|token| token.is_symbol(')')) {
                index += 1;
            }
        } else if let Some(name) = tokens[index].ident() {
            normalized.push(Token {
                kind: TokenKind::Number(values.get(name).copied().unwrap_or(0)),
                line: tokens[index].line,
            });
            index += 1;
        } else {
            normalized.push(tokens[index].clone());
            index += 1;
        }
    }

    evaluate_constant(&normalized, &HashMap::new()).is_some_and(|value| value != 0)
}

fn collect_macro_definitions(source: &str) -> Result<HashMap<String, MacroDefinition>, String> {
    let mut definitions = HashMap::new();

    for (line_index, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("define") else {
            continue;
        };
        let rest = rest.trim_start();
        let name_len = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .count();
        if name_len == 0 {
            continue;
        }

        let name = rest[..name_len].to_string();
        let suffix = &rest[name_len..];
        let (params, replacement) = if suffix.starts_with('(') {
            let Some(close) = matching_macro_parenthesis(suffix) else {
                return Err(format!(
                    "第 {} 行的宏 `{name}` 参数列表未闭合。",
                    line_index + 1
                ));
            };
            let raw_params = &suffix[1..close];
            let mut params = Vec::new();
            let mut valid = true;
            for param in raw_params.split(',').map(str::trim) {
                if param.is_empty() {
                    continue;
                }
                if !is_c_identifier(param) {
                    valid = false;
                    break;
                }
                params.push(param.to_string());
            }
            if !valid {
                continue;
            }
            (Some(params), suffix[close + 1..].trim())
        } else {
            (None, suffix.trim())
        };

        let mut replacement_tokens = tokenize(replacement)?;
        for token in &mut replacement_tokens {
            token.line = line_index + 1;
        }
        definitions.insert(
            name,
            MacroDefinition {
                params,
                replacement: replacement_tokens,
            },
        );
    }

    Ok(definitions)
}

fn matching_macro_parenthesis(value: &str) -> Option<usize> {
    let mut depth = 0_i32;
    for (index, ch) in value.char_indices() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

fn is_c_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn expand_macro_tokens(
    tokens: &[Token],
    definitions: &HashMap<String, MacroDefinition>,
) -> Result<Vec<Token>, String> {
    expand_macro_tokens_inner(tokens, definitions, &mut HashSet::new(), 0)
}

fn expand_macro_tokens_inner(
    tokens: &[Token],
    definitions: &HashMap<String, MacroDefinition>,
    active: &mut HashSet<String>,
    depth: usize,
) -> Result<Vec<Token>, String> {
    if depth >= MAX_MACRO_EXPANSION_DEPTH {
        return Err("宏展开层级过深，请检查宏是否存在循环引用。".to_string());
    }

    let mut result = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let Some(name) = token.ident() else {
            result.push(token.clone());
            index += 1;
            continue;
        };
        let Some(definition) = definitions.get(name) else {
            result.push(token.clone());
            index += 1;
            continue;
        };
        if active.contains(name) {
            result.push(token.clone());
            index += 1;
            continue;
        }

        let (replacement, consumed) = if let Some(params) = &definition.params {
            if !tokens
                .get(index + 1)
                .is_some_and(|next| next.is_symbol('('))
            {
                result.push(token.clone());
                index += 1;
                continue;
            }
            let Some((arguments, close_index)) = macro_arguments(tokens, index + 1) else {
                result.push(token.clone());
                index += 1;
                continue;
            };
            if arguments.len() != params.len() {
                result.push(token.clone());
                index += 1;
                continue;
            }

            let mut substitutions = HashMap::new();
            for (param, argument) in params.iter().zip(arguments) {
                let expanded =
                    expand_macro_tokens_inner(&argument, definitions, active, depth + 1)?;
                substitutions.insert(param.as_str(), expanded);
            }
            let mut replacement = Vec::new();
            for replacement_token in &definition.replacement {
                if let Some(value) = replacement_token
                    .ident()
                    .and_then(|ident| substitutions.get(ident))
                {
                    replacement.extend(value.iter().cloned());
                } else {
                    replacement.push(replacement_token.clone());
                }
            }
            (replacement, close_index - index + 1)
        } else {
            (definition.replacement.clone(), 1)
        };

        let replacement: Vec<Token> = replacement
            .into_iter()
            .map(|mut replacement_token| {
                replacement_token.line = token.line;
                replacement_token
            })
            .collect();
        active.insert(name.to_string());
        let expanded = expand_macro_tokens_inner(&replacement, definitions, active, depth + 1)?;
        active.remove(name);
        result.extend(expanded);
        if result.len() > MAX_MACRO_EXPANSION_TOKENS {
            return Err(format!(
                "宏展开后超过 {MAX_MACRO_EXPANSION_TOKENS} 个标记，请缩小宏展开规模。"
            ));
        }
        index += consumed;
    }
    Ok(result)
}

fn macro_arguments(tokens: &[Token], open_index: usize) -> Option<(Vec<Vec<Token>>, usize)> {
    if !tokens.get(open_index)?.is_symbol('(') {
        return None;
    }
    let mut arguments = Vec::new();
    let mut current = Vec::new();
    let mut depth = 1_i32;
    let mut index = open_index + 1;
    while index < tokens.len() {
        if tokens[index].is_symbol('(') {
            depth += 1;
            current.push(tokens[index].clone());
        } else if tokens[index].is_symbol(')') {
            depth -= 1;
            if depth == 0 {
                if !current.is_empty() || !arguments.is_empty() {
                    arguments.push(current);
                }
                return Some((arguments, index));
            }
            current.push(tokens[index].clone());
        } else if tokens[index].is_symbol(',') && depth == 1 {
            arguments.push(std::mem::take(&mut current));
        } else {
            current.push(tokens[index].clone());
        }
        index += 1;
    }
    None
}

fn collect_integer_macros(source: &str) -> HashMap<String, u64> {
    let joined = source.replace("\\\r\n", " ").replace("\\\n", " ");
    let mut pending = Vec::new();
    for line in joined.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('#') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix("define") else {
            continue;
        };
        let rest = rest.trim_start();
        let name_len = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .count();
        if name_len == 0 || rest[name_len..].starts_with('(') {
            continue;
        }
        let name = rest[..name_len].to_string();
        let expression = rest[name_len..]
            .split("//")
            .next()
            .unwrap_or("")
            .split("/*")
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !expression.is_empty() {
            pending.push((name, expression));
        }
    }

    let mut values = HashMap::new();
    for _ in 0..32 {
        let mut changed = false;
        for (name, expression) in &pending {
            if values.contains_key(name) {
                continue;
            }
            if let Ok(tokens) = tokenize(expression) {
                if let Some(value) = evaluate_constant(&tokens, &values) {
                    values.insert(name.clone(), value);
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    values
}

struct ConstantParser<'a> {
    tokens: &'a [Token],
    index: usize,
    values: &'a HashMap<String, u64>,
}

fn evaluate_constant(tokens: &[Token], values: &HashMap<String, u64>) -> Option<u64> {
    let code: Vec<Token> = tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Comment(_)))
        .cloned()
        .collect();
    let mut parser = ConstantParser {
        tokens: &code,
        index: 0,
        values,
    };
    let value = parser.parse_logical_or()?;
    (parser.index == code.len()).then_some(value)
}

impl ConstantParser<'_> {
    fn take_symbol(&mut self, symbol: char) -> bool {
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.is_symbol(symbol))
        {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn take_pair(&mut self, first: char, second: char) -> bool {
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.is_symbol(first))
            && self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is_symbol(second))
        {
            self.index += 2;
            true
        } else {
            false
        }
    }

    fn take_single(&mut self, symbol: char) -> bool {
        if self
            .tokens
            .get(self.index)
            .is_some_and(|token| token.is_symbol(symbol))
            && !self
                .tokens
                .get(self.index + 1)
                .is_some_and(|token| token.is_symbol(symbol) || token.is_symbol('='))
        {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn parse_logical_or(&mut self) -> Option<u64> {
        let mut value = self.parse_logical_and()?;
        while self.take_pair('|', '|') {
            let right = self.parse_logical_and()?;
            value = u64::from(value != 0 || right != 0);
        }
        Some(value)
    }

    fn parse_logical_and(&mut self) -> Option<u64> {
        let mut value = self.parse_bit_or()?;
        while self.take_pair('&', '&') {
            let right = self.parse_bit_or()?;
            value = u64::from(value != 0 && right != 0);
        }
        Some(value)
    }

    fn parse_bit_or(&mut self) -> Option<u64> {
        let mut value = self.parse_xor()?;
        while self.take_single('|') {
            value |= self.parse_xor()?;
        }
        Some(value)
    }

    fn parse_xor(&mut self) -> Option<u64> {
        let mut value = self.parse_bit_and()?;
        while self.take_symbol('^') {
            value ^= self.parse_bit_and()?;
        }
        Some(value)
    }

    fn parse_bit_and(&mut self) -> Option<u64> {
        let mut value = self.parse_equality()?;
        while self.take_single('&') {
            value &= self.parse_equality()?;
        }
        Some(value)
    }

    fn parse_equality(&mut self) -> Option<u64> {
        let mut value = self.parse_relational()?;
        loop {
            if self.take_pair('=', '=') {
                value = u64::from(value == self.parse_relational()?);
            } else if self.take_pair('!', '=') {
                value = u64::from(value != self.parse_relational()?);
            } else {
                break;
            }
        }
        Some(value)
    }

    fn parse_relational(&mut self) -> Option<u64> {
        let mut value = self.parse_shift()?;
        loop {
            if self.take_pair('<', '=') {
                value = u64::from(value <= self.parse_shift()?);
            } else if self.take_pair('>', '=') {
                value = u64::from(value >= self.parse_shift()?);
            } else if self.take_single('<') {
                value = u64::from(value < self.parse_shift()?);
            } else if self.take_single('>') {
                value = u64::from(value > self.parse_shift()?);
            } else {
                break;
            }
        }
        Some(value)
    }

    fn parse_shift(&mut self) -> Option<u64> {
        let mut value = self.parse_add()?;
        loop {
            if self.take_pair('<', '<') {
                value = value.checked_shl(self.parse_add()? as u32)?;
            } else if self.take_pair('>', '>') {
                value = value.checked_shr(self.parse_add()? as u32)?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn parse_add(&mut self) -> Option<u64> {
        let mut value = self.parse_mul()?;
        loop {
            if self.take_symbol('+') {
                value = value.checked_add(self.parse_mul()?)?;
            } else if self.take_symbol('-') {
                value = value.checked_sub(self.parse_mul()?)?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn parse_mul(&mut self) -> Option<u64> {
        let mut value = self.parse_unary()?;
        loop {
            if self.take_symbol('*') {
                value = value.checked_mul(self.parse_unary()?)?;
            } else if self.take_symbol('/') {
                value = value.checked_div(self.parse_unary()?)?;
            } else if self.take_symbol('%') {
                value = value.checked_rem(self.parse_unary()?)?;
            } else {
                break;
            }
        }
        Some(value)
    }

    fn parse_unary(&mut self) -> Option<u64> {
        if self.take_symbol('+') {
            self.parse_unary()
        } else if self.take_symbol('-') {
            Some(0_u64.wrapping_sub(self.parse_unary()?))
        } else if self.take_symbol('~') {
            Some(!self.parse_unary()?)
        } else if self.take_symbol('!') {
            Some(u64::from(self.parse_unary()? == 0))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Option<u64> {
        let token = self.tokens.get(self.index)?;
        match &token.kind {
            TokenKind::Number(value) => {
                self.index += 1;
                Some(*value)
            }
            TokenKind::Ident(name) => {
                self.index += 1;
                self.values.get(name).copied()
            }
            TokenKind::Symbol('(') => {
                self.index += 1;
                let value = self.parse_logical_or()?;
                self.take_symbol(')').then_some(value)
            }
            _ => None,
        }
    }
}

fn matching_brace(tokens: &[Token], open: usize) -> Result<usize, String> {
    let mut depth = 0_u32;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        if token.is_symbol('{') {
            depth += 1;
        } else if token.is_symbol('}') {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Ok(index);
            }
        }
    }
    Err("存在未闭合的结构体大括号。".to_string())
}

fn parse_fields(
    body: &[Token],
    known_structs: &HashSet<String>,
    aliases: &HashMap<String, String>,
    type_aliases: &HashMap<String, TypeAlias>,
    macro_values: &HashMap<String, u64>,
) -> Vec<FieldDef> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut brace_depth = 0_i32;
    let mut paren_depth = 0_i32;

    for (index, token) in body.iter().enumerate() {
        if token.is_symbol('{') {
            brace_depth += 1;
        } else if token.is_symbol('}') {
            brace_depth -= 1;
        } else if token.is_symbol('(') {
            paren_depth += 1;
        } else if token.is_symbol(')') {
            paren_depth -= 1;
        } else if token.is_symbol(';') && brace_depth == 0 && paren_depth == 0 {
            let mut declaration = body[start..index].to_vec();
            let mut next_start = index + 1;
            while let Some(next) = body.get(next_start) {
                if matches!(next.kind, TokenKind::Comment(_)) && next.line == token.line {
                    declaration.push(next.clone());
                    next_start += 1;
                } else {
                    break;
                }
            }
            parse_declaration(
                &declaration,
                known_structs,
                aliases,
                type_aliases,
                macro_values,
                &mut fields,
            );
            start = next_start;
        }
    }
    fields
}

fn parse_declaration(
    declaration: &[Token],
    known_structs: &HashSet<String>,
    aliases: &HashMap<String, String>,
    type_aliases: &HashMap<String, TypeAlias>,
    macro_values: &HashMap<String, u64>,
    fields: &mut Vec<FieldDef>,
) {
    let comment = declaration
        .iter()
        .filter_map(|token| match &token.kind {
            TokenKind::Comment(value) if !value.is_empty() => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    let code: Vec<Token> = remove_attributes(declaration)
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Comment(_)))
        .cloned()
        .collect();

    if code.is_empty()
        || code.iter().any(|token| token.is_symbol('{'))
        || code.iter().any(|token| token.is_ident("static_assert"))
    {
        return;
    }

    let segments = split_on_commas(&code);
    let Some(first) = segments.first() else {
        return;
    };
    let Some(first_name_index) = declarator_name_index(first) else {
        return;
    };

    let base_tokens = &first[..first_name_index];
    let (type_name, first_pointer_depth) =
        normalize_type(base_tokens, known_structs, aliases, type_aliases);
    if type_name.is_empty() {
        return;
    }

    for (segment_index, segment) in segments.iter().enumerate() {
        let Some(name_index) = declarator_name_index(segment) else {
            continue;
        };
        let Some(name) = segment[name_index].ident() else {
            continue;
        };
        let pointer_depth = if segment_index == 0 {
            first_pointer_depth
        } else {
            segment[..name_index]
                .iter()
                .filter(|token| token.is_symbol('*'))
                .count()
        };
        let array_len = array_length(&segment[name_index + 1..], macro_values);
        let bit_width = bit_width(&segment[name_index + 1..], macro_values);

        fields.push(FieldDef {
            name: name.to_string(),
            type_name: type_name.clone(),
            pointer_depth,
            array_len,
            bit_width,
            comment: comment.clone(),
        });
    }
}

fn split_on_commas(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut bracket_depth = 0_i32;
    let mut paren_depth = 0_i32;
    for (index, token) in tokens.iter().enumerate() {
        if token.is_symbol('[') {
            bracket_depth += 1;
        } else if token.is_symbol(']') {
            bracket_depth -= 1;
        } else if token.is_symbol('(') {
            paren_depth += 1;
        } else if token.is_symbol(')') {
            paren_depth -= 1;
        } else if token.is_symbol(',') && bracket_depth == 0 && paren_depth == 0 {
            result.push(tokens[start..index].to_vec());
            start = index + 1;
        }
    }
    result.push(tokens[start..].to_vec());
    result
}

fn declarator_name_index(tokens: &[Token]) -> Option<usize> {
    let end = tokens
        .iter()
        .position(|token| token.is_symbol('[') || token.is_symbol(':'))
        .unwrap_or(tokens.len());
    (0..end).rev().find(|index| {
        tokens[*index]
            .ident()
            .is_some_and(|value| !is_type_keyword(value) && !value.starts_with("__"))
    })
}

fn normalize_type(
    tokens: &[Token],
    known_structs: &HashSet<String>,
    aliases: &HashMap<String, String>,
    type_aliases: &HashMap<String, TypeAlias>,
) -> (String, usize) {
    let mut pointer_depth = tokens.iter().filter(|token| token.is_symbol('*')).count();
    let words: Vec<&str> = tokens
        .iter()
        .filter_map(Token::ident)
        .filter(|word| !is_qualifier(word) && !is_attribute_word(word) && !is_annotation_word(word))
        .collect();

    if words.first() == Some(&"struct") || words.first() == Some(&"union") {
        if let Some(name) = words.get(1) {
            return (
                aliases
                    .get(*name)
                    .cloned()
                    .unwrap_or_else(|| (*name).to_string()),
                pointer_depth,
            );
        }
    }
    if words.first() == Some(&"enum") {
        return ("enum".to_string(), pointer_depth);
    }

    if let Some(known) = words
        .iter()
        .find(|word| known_structs.contains(**word))
        .and_then(|word| aliases.get(*word))
    {
        return (known.clone(), pointer_depth);
    }

    let mut type_name = words
        .iter()
        .rev()
        .find(|word| type_aliases.contains_key(**word))
        .map(|word| (*word).to_string())
        .unwrap_or_else(|| words.join(" "));
    let mut visited = HashSet::new();
    while let Some(type_alias) = type_aliases.get(&type_name) {
        if !visited.insert(type_name.clone()) {
            break;
        }
        pointer_depth += type_alias.pointer_depth;
        type_name = type_alias.target.clone();
    }
    if let Some(canonical) = aliases.get(&type_name) {
        type_name = canonical.clone();
    }
    (type_name, pointer_depth)
}

fn is_annotation_word(value: &str) -> bool {
    (value.starts_with('_') && value.ends_with('_'))
        || matches!(value, "IN" | "OUT" | "INOUT" | "OPTIONAL" | "FAR" | "NEAR")
}

fn is_qualifier(value: &str) -> bool {
    matches!(
        value,
        "const"
            | "volatile"
            | "static"
            | "extern"
            | "register"
            | "auto"
            | "restrict"
            | "_Atomic"
            | "typedef"
    )
}

fn is_type_keyword(value: &str) -> bool {
    is_qualifier(value)
        || matches!(
            value,
            "struct"
                | "enum"
                | "union"
                | "signed"
                | "unsigned"
                | "short"
                | "long"
                | "int"
                | "char"
                | "float"
                | "double"
                | "void"
                | "_Bool"
                | "bool"
        )
}

fn array_length(tokens: &[Token], macro_values: &HashMap<String, u64>) -> u64 {
    let mut length = 1_u64;
    let mut found = false;
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index].is_symbol('[') {
            found = true;
            let mut close = index + 1;
            let mut depth = 1_i32;
            while close < tokens.len() && depth > 0 {
                if tokens[close].is_symbol('[') {
                    depth += 1;
                } else if tokens[close].is_symbol(']') {
                    depth -= 1;
                }
                if depth > 0 {
                    close += 1;
                }
            }
            let dimension = evaluate_constant(&tokens[index + 1..close], macro_values).unwrap_or(1);
            length = length.saturating_mul(dimension.max(1));
            index = close;
        }
        index += 1;
    }
    if found {
        length
    } else {
        1
    }
}

fn bit_width(tokens: &[Token], macro_values: &HashMap<String, u64>) -> Option<u64> {
    tokens
        .iter()
        .position(|token| token.is_symbol(':'))
        .and_then(|index| evaluate_constant(&tokens[index + 1..], macro_values))
}

fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment <= 1 {
        value
    } else {
        (value + alignment - 1) / alignment * alignment
    }
}

fn struct_layout(
    name: &str,
    definitions: &HashMap<String, StructDef>,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, TypeLayout>,
    visiting: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) -> Result<TypeLayout, String> {
    let canonical = aliases.get(name).map(String::as_str).unwrap_or(name);
    if let Some(layout) = cache.get(canonical) {
        return Ok(layout.clone());
    }
    if !visiting.insert(canonical.to_string()) {
        return Err(format!(
            "结构体 `{canonical}` 存在按值递归引用；请改为指针或检查定义。"
        ));
    }

    let definition = definitions
        .get(canonical)
        .ok_or_else(|| format!("缺少结构体 `{canonical}` 的定义。"))?;
    let (_, size, max_align) =
        layout_fields(definition, definitions, aliases, cache, visiting, warnings)?;
    visiting.remove(canonical);

    let layout = TypeLayout {
        size,
        align: max_align,
        display_name: canonical.to_string(),
        nested_name: Some(canonical.to_string()),
    };
    cache.insert(canonical.to_string(), layout.clone());
    Ok(layout)
}

fn layout_fields(
    definition: &StructDef,
    definitions: &HashMap<String, StructDef>,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, TypeLayout>,
    visiting: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) -> Result<(Vec<FieldPlacement>, u64, u64), String> {
    let mut placements = Vec::with_capacity(definition.fields.len());

    if definition.is_union {
        let mut union_size = 0_u64;
        let mut union_align = 1_u64;
        for field in &definition.fields {
            let layout = field_layout(field, definitions, aliases, cache, visiting, warnings)?;
            let effective_align = definition
                .pack
                .map(|pack| layout.align.min(pack))
                .unwrap_or(layout.align)
                .max(1);
            union_size = union_size.max(layout.size);
            union_align = union_align.max(effective_align);
            placements.push(FieldPlacement { offset: 0, layout });
        }
        return Ok((placements, align_up(union_size, union_align), union_align));
    }

    let mut offset = 0_u64;
    let mut max_align = 1_u64;
    // start offset, storage size, bits already used
    let mut bit_unit: Option<(u64, u64, u64)> = None;

    for field in &definition.fields {
        let layout = field_layout(field, definitions, aliases, cache, visiting, warnings)?;
        let effective_align = definition
            .pack
            .map(|pack| layout.align.min(pack))
            .unwrap_or(layout.align)
            .max(1);
        max_align = max_align.max(effective_align);

        let field_offset = if let Some(width) = field.bit_width {
            let storage_bits = layout.size.saturating_mul(8);
            let reuse = bit_unit.is_some_and(|(_, size, used)| {
                size == layout.size && width > 0 && used.saturating_add(width) <= storage_bits
            });
            if reuse {
                let (start, _, used) = bit_unit.expect("bit unit exists");
                bit_unit = Some((start, layout.size, used + width));
                start
            } else {
                offset = align_up(offset, effective_align);
                let start = offset;
                if width == 0 {
                    bit_unit = None;
                } else {
                    offset = offset.saturating_add(layout.size);
                    bit_unit = Some((start, layout.size, width.min(storage_bits)));
                }
                start
            }
        } else {
            bit_unit = None;
            offset = align_up(offset, effective_align);
            let start = offset;
            offset = offset.saturating_add(layout.size);
            start
        };
        placements.push(FieldPlacement {
            offset: field_offset,
            layout,
        });
    }

    let struct_align = max_align.max(1);
    Ok((placements, align_up(offset, struct_align), struct_align))
}

fn field_layout(
    field: &FieldDef,
    definitions: &HashMap<String, StructDef>,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, TypeLayout>,
    visiting: &mut HashSet<String>,
    warnings: &mut Vec<String>,
) -> Result<TypeLayout, String> {
    let mut layout = if field.pointer_depth > 0 {
        TypeLayout {
            size: 8,
            align: 8,
            display_name: "POINTER".to_string(),
            nested_name: None,
        }
    } else if let Some(canonical) = aliases.get(&field.type_name) {
        struct_layout(canonical, definitions, aliases, cache, visiting, warnings)?
    } else {
        primitive_layout(&field.type_name, warnings)
    };

    layout.size = layout.size.saturating_mul(field.array_len.max(1));
    if field.array_len > 1 {
        layout.display_name = format!("{}[{}]", layout.display_name, field.array_len);
    }
    Ok(layout)
}

fn primitive_layout(type_name: &str, warnings: &mut Vec<String>) -> TypeLayout {
    let normalized = type_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    let (size, display) = match normalized.as_str() {
        "bool" | "_bool" => (1, "BOOL"),
        "char" | "signed char" | "int8_t" => (1, "INT8"),
        "unsigned char" | "uint8_t" | "byte" => (1, "UINT8"),
        "short" | "short int" | "signed short" | "signed short int" | "int16_t" => (2, "INT16"),
        "unsigned short" | "unsigned short int" | "uint16_t" => (2, "UINT16"),
        "int" | "signed" | "signed int" | "int32_t" | "enum" => (4, "INT"),
        "unsigned" | "unsigned int" | "uint32_t" => (4, "UINT"),
        "long" | "long int" | "signed long" | "signed long int" => (4, "LONG"),
        "unsigned long" | "unsigned long int" => (4, "ULONG"),
        "long long" | "long long int" | "signed long long" | "int64_t" => (8, "INT64"),
        "unsigned long long" | "unsigned long long int" | "uint64_t" => (8, "UINT64"),
        "float" => (4, "FLOAT"),
        "double" | "long double" => (8, "DOUBLE"),
        "size_t" | "uintptr_t" | "intptr_t" | "ptrdiff_t" => (8, "UINT64"),
        _ => {
            let digits = normalized
                .chars()
                .filter(char::is_ascii_digit)
                .collect::<String>();
            let guessed = match digits.as_str() {
                "8" => Some(1),
                "16" => Some(2),
                "32" => Some(4),
                "64" => Some(8),
                _ => None,
            };
            let size = guessed.unwrap_or(4);
            warnings.push(format!(
                "类型 `{type_name}` 未知，暂按 {} 位整数计算。",
                size * 8
            ));
            let display = if normalized.starts_with('u') || normalized.contains("uint") {
                format!("UINT{}", size * 8)
            } else {
                format!("INT{}", size * 8)
            };
            return TypeLayout {
                size,
                align: size.min(8),
                display_name: display,
                nested_name: None,
            };
        }
    };

    TypeLayout {
        size,
        align: size.min(8),
        display_name: display.to_string(),
        nested_name: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn flatten_struct(
    struct_name: &str,
    prefix: &str,
    base_offset: u64,
    definitions: &HashMap<String, StructDef>,
    aliases: &HashMap<String, String>,
    cache: &mut HashMap<String, TypeLayout>,
    warnings: &mut Vec<String>,
    rows: &mut Vec<FieldRow>,
) -> Result<(), String> {
    let definition = definitions
        .get(struct_name)
        .ok_or_else(|| format!("缺少结构体 `{struct_name}` 的定义。"))?;
    let mut visiting = HashSet::new();
    let (placements, _, _) = layout_fields(
        definition,
        definitions,
        aliases,
        cache,
        &mut visiting,
        warnings,
    )?;

    for (field, placement) in definition.fields.iter().zip(placements) {
        let layout = placement.layout;
        let field_offset = base_offset + placement.offset;
        let path = if prefix.is_empty() {
            field.name.clone()
        } else {
            format!("{prefix}.{}", field.name)
        };

        if let Some(nested_name) = &layout.nested_name {
            let element_layout = cache
                .get(nested_name)
                .cloned()
                .ok_or_else(|| format!("无法计算 `{nested_name}` 的布局。"))?;
            for array_index in 0..field.array_len.max(1) {
                let nested_path = if field.array_len > 1 {
                    format!("{path}[{array_index}]")
                } else {
                    path.clone()
                };
                flatten_struct(
                    nested_name,
                    &nested_path,
                    field_offset + array_index * element_layout.size,
                    definitions,
                    aliases,
                    cache,
                    warnings,
                    rows,
                )?;
            }
        } else {
            if rows.len() >= MAX_EXPANDED_ROWS {
                return Err(format!(
                    "展开后的字段超过 {MAX_EXPANDED_ROWS} 行，请缩小数组或结构体规模。"
                ));
            }
            let bit_size = field
                .bit_width
                .unwrap_or_else(|| layout.size.saturating_mul(8));
            rows.push(FieldRow {
                status: "Initial".to_string(),
                parameter_name: field.name.clone(),
                system_parameter_name: field.name.clone(),
                data_type: layout.display_name,
                data_size: bit_size,
                in_io_buffer: "Yes".to_string(),
                io_buffer_offset: field_offset,
                comments: field.comment.clone(),
                group_key: if prefix.is_empty() {
                    format!("root:{struct_name}")
                } else {
                    prefix.to_string()
                },
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_flattens_nested_structs() {
        let source = r#"
            typedef struct {
                uint16_t code; // 状态码
                float voltage;
            } Sensor;

            typedef struct Device {
                uint32_t id;
                Sensor sensors[2];
                char name[8];
            } Device;
        "#;

        let result = parse(source, Some("Device")).unwrap();
        assert_eq!(result.root_name, "Device");
        assert_eq!(result.total_size, 28);
        assert_eq!(result.rows.len(), 6);
        assert_eq!(result.rows[1].parameter_name, "code");
        assert_eq!(
            result.rows[1].system_parameter_name,
            result.rows[1].parameter_name
        );
        assert_eq!(result.rows[1].comments, "状态码");
        assert_eq!(result.rows[1].group_key, result.rows[2].group_key);
        assert_ne!(result.rows[1].group_key, result.rows[3].group_key);
        assert_eq!(result.rows[0].group_key, result.rows[5].group_key);
        assert_eq!(result.rows[3].io_buffer_offset, 12);
        assert_eq!(result.rows[5].data_size, 64);
    }

    #[test]
    fn supports_named_struct_references_and_pointers() {
        let source = r#"
            struct Node {
                int value;
                struct Node *next;
            };
        "#;
        let result = parse(source, None).unwrap();
        assert_eq!(result.total_size, 16);
        assert_eq!(result.rows[1].data_type, "POINTER");
        assert_eq!(result.rows[1].io_buffer_offset, 8);
    }

    #[test]
    fn uses_last_struct_as_default_root() {
        let source = "typedef struct { int x; } A; typedef struct { A a; int y; } B;";
        let result = parse(source, None).unwrap();
        assert_eq!(result.root_name, "B");
        assert_eq!(result.rows[0].parameter_name, "x");
    }

    #[test]
    fn parses_realistic_header_and_allows_any_root() {
        let source = r#"
            #ifndef DEVICE_PROTOCOL_H
            #define DEVICE_PROTOCOL_H
            #include <stdint.h>
            #define CHANNEL_COUNT (1U << 1)
            #define DEFAULT_SCALE 1.0f

            typedef unsigned char U8;
            typedef unsigned int U32;

            typedef struct __attribute__((packed)) _Sensor {
                U8 status;
                uint16_t code;
            } Sensor, *PSensor;

            typedef struct {
                U32 id;
                Sensor sensors[CHANNEL_COUNT];
            } Packet, *PPacket;

            int decode_packet(const Packet *packet);
            #endif
        "#;

        let catalog = discover(source).unwrap();
        assert_eq!(catalog.available_structs, vec!["Sensor", "Packet"]);
        assert_eq!(catalog.default_root, "Packet");

        let sensor = parse(source, Some("Sensor")).unwrap();
        assert_eq!(sensor.root_name, "Sensor");
        assert_eq!(sensor.rows.len(), 2);
        assert_eq!(sensor.rows[0].data_size, 8);
        assert_eq!(sensor.rows[1].io_buffer_offset, 1);

        let packet = parse(source, Some("Packet")).unwrap();
        assert_eq!(packet.rows.len(), 5);
        assert_eq!(packet.rows[1].parameter_name, "status");
        assert_eq!(packet.rows[3].parameter_name, "status");
        assert_eq!(packet.total_size, 12);
    }

    #[test]
    fn expands_object_function_and_multiline_macros_inside_structs() {
        let source = r#"
            #define BASE_COUNT 2U
            #define CHANNEL_COUNT (BASE_COUNT * 2U)
            #define FIELD(type, name) type name;
            #define ARRAY_FIELD(type, name, count) \
                type name[count];
            #define COMMON_FIELDS \
                FIELD(uint32_t, id) \
                FIELD(uint8_t, mode)
            #define FLAG_BITS (1U + 2U)

            typedef struct {
                COMMON_FIELDS
                ARRAY_FIELD(uint16_t, samples, CHANNEL_COUNT)
                unsigned int flags : FLAG_BITS;
            } MacroRecord;
        "#;

        let result = parse(source, Some("MacroRecord")).unwrap();
        assert_eq!(result.rows.len(), 4);
        assert_eq!(result.rows[0].parameter_name, "id");
        assert_eq!(result.rows[1].parameter_name, "mode");
        assert_eq!(result.rows[2].parameter_name, "samples");
        assert_eq!(result.rows[2].data_type, "UINT16[4]");
        assert_eq!(result.rows[2].data_size, 64);
        assert_eq!(result.rows[2].io_buffer_offset, 6);
        assert_eq!(result.rows[3].parameter_name, "flags");
        assert_eq!(result.rows[3].data_size, 3);
        assert_eq!(result.rows[3].io_buffer_offset, 16);
        assert_eq!(result.total_size, 20);
    }

    #[test]
    fn honors_conditional_macros_inside_structs() {
        let source = r#"
            #define ENABLE_EXTRA 1
            #define MODE 2

            typedef struct {
                uint8_t prefix;
            #if ENABLE_EXTRA && MODE >= 2
                uint16_t enabled_value;
            #else
                uint64_t disabled_value;
            #endif
            #if 0
                uint32_t omitted_value;
            #endif
            #if defined(_WIN64) && !defined(NOT_DEFINED)
                uint32_t windows_value;
            #endif
            } ConditionalRecord;
        "#;

        let result = parse(source, Some("ConditionalRecord")).unwrap();
        assert_eq!(result.rows.len(), 3);
        assert_eq!(result.rows[0].parameter_name, "prefix");
        assert_eq!(result.rows[1].parameter_name, "enabled_value");
        assert_eq!(result.rows[1].io_buffer_offset, 2);
        assert_eq!(result.rows[2].parameter_name, "windows_value");
        assert_eq!(result.rows[2].io_buffer_offset, 4);
        assert_eq!(result.total_size, 8);
    }

    #[test]
    fn honors_pragma_pack_and_packs_bitfields() {
        let source = r#"
            #pragma pack(push, 1)
            typedef struct {
                uint8_t first;
                uint32_t value;
                uint16_t last;
            } PackedRecord;

            typedef struct {
                uint8_t prefix;
                unsigned int low : 3;
                unsigned int high : 5;
                uint8_t suffix;
            } PackedBits;
            #pragma pack(pop)

            typedef struct {
                uint8_t first;
                uint32_t value;
                uint16_t last;
            } NaturalRecord;
        "#;

        let packed = parse(source, Some("PackedRecord")).unwrap();
        assert_eq!(packed.total_size, 7);
        assert_eq!(packed.rows[1].io_buffer_offset, 1);
        assert_eq!(packed.rows[2].io_buffer_offset, 5);

        let bits = parse(source, Some("PackedBits")).unwrap();
        assert_eq!(bits.total_size, 6);
        assert_eq!(bits.rows[1].io_buffer_offset, 1);
        assert_eq!(bits.rows[2].io_buffer_offset, 1);
        assert_eq!(bits.rows[1].data_size, 3);
        assert_eq!(bits.rows[2].data_size, 5);
        assert_eq!(bits.rows[3].io_buffer_offset, 5);

        let natural = parse(source, Some("NaturalRecord")).unwrap();
        assert_eq!(natural.total_size, 12);
        assert_eq!(natural.rows[1].io_buffer_offset, 4);
        assert_eq!(natural.rows[2].io_buffer_offset, 8);
    }

    #[test]
    fn uses_union_size_when_calculating_struct_offsets() {
        let source = r#"
            typedef union {
                uint8_t byte_value;
                uint64_t wide_value;
            } Value;

            typedef struct {
                uint8_t tag;
                Value value;
                uint16_t tail;
            } WithUnion;
        "#;

        let catalog = discover(source).unwrap();
        assert_eq!(catalog.available_structs, vec!["WithUnion"]);
        let result = parse(source, Some("WithUnion")).unwrap();
        assert_eq!(result.total_size, 24);
        assert_eq!(result.rows[1].io_buffer_offset, 8);
        assert_eq!(result.rows[2].io_buffer_offset, 8);
        assert_eq!(result.rows[3].io_buffer_offset, 16);
    }
}
