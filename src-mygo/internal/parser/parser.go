// Package parser 把 C 头文件里的结构体定义解析成字段表行。它是
// src-gpui/src/parser.rs 的 Go 移植，行为按 64 位 Windows C ABI 对齐。
package parser

import (
	"fmt"
	"sort"
	"strings"
	"unicode"
)

const (
	maxExpandedRows         = 10_000
	maxMacroExpansionTokens = 200_000
	maxMacroExpansionDepth  = 64
)

// FieldRow 是导出字段表中的一行（JSON 采用 camelCase 与旧前端契约一致）。
type FieldRow struct {
	Status              string `json:"status"`
	ParameterName       string `json:"parameterName"`
	SystemParameterName string `json:"systemParameterName"`
	DataType            string `json:"dataType"`
	DataSize            uint64 `json:"dataSize"`
	InIOBuffer          string `json:"inIoBuffer"`
	IOBufferOffset      uint64 `json:"ioBufferOffset"`
	Comments            string `json:"comments"`

	GroupKey string `json:"-"`
}

// ParseResult 是一次解析的完整输出。
type ParseResult struct {
	RootName         string     `json:"rootName"`
	AvailableStructs []string   `json:"availableStructs"`
	TotalSize        uint64     `json:"totalSize"`
	Rows             []FieldRow `json:"rows"`
	Warnings         []string   `json:"warnings"`
}

// StructCatalog 是 discover 的输出：可导出的结构体列表与默认根。
type StructCatalog struct {
	AvailableStructs []string `json:"availableStructs"`
	DefaultRoot      string   `json:"defaultRoot"`
}

type tokenKind int

const (
	tokenIdent tokenKind = iota
	tokenNumber
	tokenSymbol
	tokenComment
)

type token struct {
	kind  tokenKind
	str   string // ident / comment 文本
	num   uint64
	sym   rune
	line  int
	index int // 词内下标（数组尺寸等场景使用）
}

func (t token) isIdent(expected string) bool {
	return t.kind == tokenIdent && t.str == expected
}

func (t token) isSymbol(expected rune) bool {
	return t.kind == tokenSymbol && t.sym == expected
}

func (t token) isComment() bool { return t.kind == tokenComment }

type structDraft struct {
	name    string
	tag     string
	hasTag  bool
	body    []token
	pack    uint64
	hasPack bool
	isUnion bool
}

type structDef struct {
	fields  []fieldDef
	pack    uint64
	hasPack bool
	isUnion bool
}

type fieldDef struct {
	name         string
	typeName     string
	pointerDepth int
	arrayLen     uint64
	bitWidth     uint64
	hasBitWidth  bool
	comment      string
}

type typeLayout struct {
	size        uint64
	align       uint64
	displayName string
	nestedName  string
	hasNested   bool
}

type fieldPlacement struct {
	offset uint64
	layout typeLayout
}

type typeAlias struct {
	target       string
	pointerDepth int
}

type macroDefinition struct {
	params      []string
	hasParams   bool
	replacement []token
}

// Discover 扫描源码中的结构体并给出可导出列表。
func Discover(source string) (*StructCatalog, error) {
	if strings.TrimSpace(source) == "" {
		return nil, fmt.Errorf("请输入 C 头文件或结构体定义。")
	}
	normalized, err := filterConditionalBlocks(joinLineContinuations(source))
	if err != nil {
		return nil, err
	}
	macros, err := collectMacroDefinitions(normalized)
	if err != nil {
		return nil, err
	}
	rawTokens, err := tokenize(normalized)
	if err != nil {
		return nil, err
	}
	tokens, err := expandMacroTokens(rawTokens, macros)
	if err != nil {
		return nil, err
	}
	packByLine := collectPackByLine(normalized)
	drafts, err := collectStructs(tokens, packByLine)
	if err != nil {
		return nil, err
	}
	if len(drafts) == 0 {
		return nil, fmt.Errorf("没有找到完整的结构体定义，请确认头文件中包含 `struct { ... }`。")
	}
	available := make([]string, 0, len(drafts))
	for _, d := range drafts {
		if !d.isUnion {
			available = append(available, d.name)
		}
	}
	if len(available) == 0 {
		return nil, fmt.Errorf("没有找到完整的结构体定义。")
	}
	return &StructCatalog{AvailableStructs: available, DefaultRoot: available[len(available)-1]}, nil
}

// Parse 解析源码并把指定根结构体（空串用默认根）展平成字段表。
func Parse(source, requestedRoot string) (*ParseResult, error) {
	if strings.TrimSpace(source) == "" {
		return nil, fmt.Errorf("请输入 C 结构体定义。")
	}

	normalized, err := filterConditionalBlocks(joinLineContinuations(source))
	if err != nil {
		return nil, err
	}
	macros, err := collectMacroDefinitions(normalized)
	if err != nil {
		return nil, err
	}
	rawTokens, err := tokenize(normalized)
	if err != nil {
		return nil, err
	}
	tokens, err := expandMacroTokens(rawTokens, macros)
	if err != nil {
		return nil, err
	}
	packByLine := collectPackByLine(normalized)
	drafts, err := collectStructs(tokens, packByLine)
	if err != nil {
		return nil, err
	}
	if len(drafts) == 0 {
		return nil, fmt.Errorf("没有找到结构体定义，请确认代码包含 `struct { ... }`。")
	}

	aliases := map[string]string{}
	for _, draft := range drafts {
		aliases[draft.name] = draft.name
		if draft.hasTag {
			aliases[draft.tag] = draft.name
		}
	}

	typeAliases := collectTypeAliases(tokens, aliases)
	macroValues := collectIntegerMacros(normalized)
	definitions := map[string]*structDef{}
	for _, draft := range drafts {
		definitions[draft.name] = &structDef{
			fields:  parseFields(draft.body, aliases, typeAliases, macroValues),
			pack:    draft.pack,
			hasPack: draft.hasPack,
			isUnion: draft.isUnion,
		}
	}

	available := make([]string, 0, len(drafts))
	for _, draft := range drafts {
		if !draft.isUnion {
			available = append(available, draft.name)
		}
	}
	if len(available) == 0 {
		return nil, fmt.Errorf("没有找到完整的结构体定义。")
	}

	rootName := available[len(available)-1]
	if trimmed := strings.TrimSpace(requestedRoot); trimmed != "" {
		canonical, ok := aliases[trimmed]
		if !ok {
			return nil, fmt.Errorf("未找到根结构体 `%s`。", trimmed)
		}
		rootName = canonical
	}

	var warnings []string
	layoutCache := map[string]typeLayout{}
	visiting := map[string]bool{}
	rootLayout, err := structLayout(rootName, definitions, aliases, layoutCache, visiting, &warnings)
	if err != nil {
		return nil, err
	}

	var rows []FieldRow
	if err := flattenStruct(rootName, "", 0, definitions, aliases, layoutCache, &warnings, &rows); err != nil {
		return nil, err
	}

	if len(rows) == 0 {
		return nil, fmt.Errorf("结构体 `%s` 中没有找到可导出的字段。", rootName)
	}

	sort.Strings(warnings)
	warnings = dedupSorted(warnings)

	return &ParseResult{
		RootName:         rootName,
		AvailableStructs: available,
		TotalSize:        rootLayout.size,
		Rows:             rows,
		Warnings:         warnings,
	}, nil
}

func dedupSorted(values []string) []string {
	if len(values) == 0 {
		return values
	}
	out := values[:1]
	for _, v := range values[1:] {
		if v != out[len(out)-1] {
			out = append(out, v)
		}
	}
	return out
}

// ---- tokenizer ----

func tokenize(source string) ([]token, error) {
	chars := []rune(source)
	var tokens []token
	index := 0
	line := 1

	for index < len(chars) {
		ch := chars[index]
		if isSpace(ch) {
			if ch == '\n' {
				line++
			}
			index++
			continue
		}

		if ch == '#' {
			for index < len(chars) && chars[index] != '\n' {
				index++
			}
			continue
		}

		if ch == '/' && index+1 < len(chars) && chars[index+1] == '/' {
			index += 2
			start := index
			for index < len(chars) && chars[index] != '\n' {
				index++
			}
			tokens = append(tokens, commentToken(string(chars[start:index]), line))
			continue
		}

		if ch == '/' && index+1 < len(chars) && chars[index+1] == '*' {
			index += 2
			start := index
			for index < len(chars) && !(chars[index] == '*' && index+1 < len(chars) && chars[index+1] == '/') {
				if chars[index] == '\n' {
					line++
				}
				index++
			}
			if index+1 >= len(chars) {
				return nil, fmt.Errorf("存在未闭合的块注释。")
			}
			tokens = append(tokens, blockCommentToken(string(chars[start:index]), line))
			index += 2
			continue
		}

		if ch == '"' || ch == '\'' {
			quote := ch
			index++
			for index < len(chars) {
				if chars[index] == '\\' {
					index += 2
				} else if chars[index] == quote {
					index++
					break
				} else {
					if chars[index] == '\n' {
						line++
					}
					index++
				}
			}
			continue
		}

		if isASCIIAlpha(ch) || ch == '_' {
			start := index
			index++
			for index < len(chars) && (isASCIIAlnum(chars[index]) || chars[index] == '_') {
				index++
			}
			tokens = append(tokens, token{kind: tokenIdent, str: string(chars[start:index]), line: line})
			continue
		}

		if isDigit(ch) {
			start := index
			index++
			for index < len(chars) && (isASCIIHexdigit(chars[index]) || chars[index] == 'x' || chars[index] == 'X' || chars[index] == 'u' || chars[index] == 'U' || chars[index] == 'l' || chars[index] == 'L') {
				index++
			}
			raw := string(chars[start:index])
			trimmed := trimSuffixes(raw, "uUlL")
			value, _ := parseCInteger(trimmed)
			tokens = append(tokens, token{kind: tokenNumber, num: value, line: line})
			continue
		}

		tokens = append(tokens, token{kind: tokenSymbol, sym: ch, line: line})
		index++
	}

	return tokens, nil
}

func isSpace(ch rune) bool {
	return ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r' || ch == '\v' || ch == '\f' || ch == 0x85 || ch == 0xA0 || unicode.IsSpace(ch)
}

func isASCIIAlpha(ch rune) bool { return (ch >= 'a' && ch <= 'z') || (ch >= 'A' && ch <= 'Z') }

func isASCIIAlnum(ch rune) bool { return isASCIIAlpha(ch) || isDigit(ch) }

func isDigit(ch rune) bool { return ch >= '0' && ch <= '9' }

func isASCIIHexdigit(ch rune) bool {
	return isDigit(ch) || (ch >= 'a' && ch <= 'f') || (ch >= 'A' && ch <= 'F')
}

func trimSuffixes(value, set string) string {
	for len(value) > 0 && strings.ContainsRune(set, rune(value[len(value)-1])) {
		value = value[:len(value)-1]
	}
	return value
}

func commentToken(text string, line int) token {
	return token{kind: tokenComment, str: strings.TrimSpace(text), line: line}
}

func blockCommentToken(text string, line int) token {
	var parts []string
	for _, l := range strings.Split(text, "\n") {
		trimmed := strings.TrimSpace(l)
		trimmed = strings.TrimSpace(strings.TrimPrefix(trimmed, "*"))
		if trimmed != "" {
			parts = append(parts, trimmed)
		}
	}
	return token{kind: tokenComment, str: strings.Join(parts, " "), line: line}
}

func parseCInteger(raw string) (uint64, bool) {
	value := strings.TrimSpace(raw)
	value = strings.TrimFunc(value, func(r rune) bool { return r == '(' || r == ')' })
	value = trimSuffixes(value, "uUlL")
	switch {
	case strings.HasPrefix(value, "0x"), strings.HasPrefix(value, "0X"):
		return parseUint(value[2:], 16)
	case strings.HasPrefix(value, "0b"), strings.HasPrefix(value, "0B"):
		return parseUint(value[2:], 2)
	case len(value) > 1 && strings.HasPrefix(value, "0"):
		if v, ok := parseUint(value[1:], 8); ok {
			return v, true
		}
		return parseUint(value, 10)
	default:
		return parseUint(value, 10)
	}
}

func parseUint(value string, base int) (uint64, bool) {
	if value == "" {
		return 0, false
	}
	var out uint64
	for _, ch := range value {
		var digit uint64
		switch {
		case isDigit(ch):
			digit = uint64(ch - '0')
		case ch >= 'a' && ch <= 'z':
			digit = uint64(ch-'a') + 10
		case ch >= 'A' && ch <= 'Z':
			digit = uint64(ch-'A') + 10
		default:
			return 0, false
		}
		if digit >= uint64(base) {
			return 0, false
		}
		next := out*uint64(base) + digit
		if next < out {
			return 0, false
		}
		out = next
	}
	return out, true
}

// ---- attributes ----

func isAttributeWord(value string) bool {
	switch value {
	case "__attribute__", "__declspec", "alignas", "_Alignas", "packed", "aligned", "PACKED":
		return true
	}
	return strings.HasPrefix(value, "__")
}

func removeAttributes(tokens []token) []token {
	cleaned := make([]token, 0, len(tokens))
	index := 0
	for index < len(tokens) {
		if tokens[index].kind == tokenIdent && isAttributeWord(tokens[index].str) {
			index++
			for index < len(tokens) && tokens[index].isComment() {
				index++
			}
			if index < len(tokens) && tokens[index].isSymbol('(') {
				depth := 0
				for index < len(tokens) {
					if tokens[index].isSymbol('(') {
						depth++
					} else if tokens[index].isSymbol(')') {
						depth--
					}
					index++
					if depth == 0 {
						break
					}
				}
			}
			continue
		}
		cleaned = append(cleaned, tokens[index])
		index++
	}
	return cleaned
}

func collectPackByLine(source string) []*uint64 {
	lines := strings.Split(source, "\n")
	values := make([]*uint64, len(lines)+3)
	var current *uint64
	var stack []*uint64

	for lineIndex, line := range lines {
		lineNumber := lineIndex + 1
		trimmed := strings.TrimLeftFunc(line, isSpace)
		directive := ""
		if rest, ok := cutPrefix(trimmed, "#"); ok {
			if rest2, ok := cutPrefix(trimLeftSpaces(rest), "pragma"); ok {
				if rest3, ok := cutPrefix(trimLeftSpaces(rest2), "pack"); ok {
					directive = trimLeftSpaces(rest3)
				}
			}
		}

		if directive != "" {
			if inner, ok := cutPrefix(directive, "("); ok {
				inner = firstSegment(inner, ")")
				parts := splitAndTrim(inner, ",")
				switch {
				case len(parts) > 0 && parts[0] == "push":
					stack = append(stack, current)
					for i := len(parts) - 1; i >= 0; i-- {
						if pack, ok := parseCInteger(parts[i]); ok && pack > 0 {
							current = &pack
							break
						}
					}
				case len(parts) > 0 && parts[0] == "pop":
					if len(stack) > 0 {
						current = stack[len(stack)-1]
						stack = stack[:len(stack)-1]
					} else {
						current = nil
					}
				case len(parts) > 0:
					if pack, ok := parseCInteger(parts[0]); ok && pack > 0 {
						current = &pack
					} else {
						current = nil
					}
				default:
					current = nil
				}
			}
		}
		values[lineNumber] = current
	}
	return values
}

func cutPrefix(value, prefix string) (string, bool) {
	if strings.HasPrefix(value, prefix) {
		return value[len(prefix):], true
	}
	return value, false
}

func trimLeftSpaces(value string) string { return strings.TrimLeftFunc(value, isSpace) }

func firstSegment(value, end string) string {
	if idx := strings.Index(value, end); idx >= 0 {
		return value[:idx]
	}
	return value
}

func splitAndTrim(value, sep string) []string {
	var parts []string
	for _, part := range strings.Split(value, sep) {
		trimmed := strings.TrimSpace(part)
		if trimmed != "" {
			parts = append(parts, trimmed)
		}
	}
	return parts
}

// ---- struct collection ----

func collectStructs(tokens []token, packByLine []*uint64) ([]structDraft, error) {
	var drafts []structDraft
	index := 0

	for index < len(tokens) {
		typedef := tokens[index].isIdent("typedef")
		structIndex := -1
		if typedef {
			candidate := index + 1
			for candidate < len(tokens) && !tokens[candidate].isSymbol(';') {
				if tokens[candidate].isIdent("struct") || tokens[candidate].isIdent("union") {
					structIndex = candidate
					break
				}
				candidate++
			}
		} else {
			structIndex = index
		}

		if structIndex < 0 {
			index++
			continue
		}
		if !tokens[structIndex].isIdent("struct") && !tokens[structIndex].isIdent("union") {
			index++
			continue
		}
		isUnion := tokens[structIndex].isIdent("union")

		cursor := structIndex + 1
		open := -1
		parenDepth := 0
		for cursor < len(tokens) {
			if tokens[cursor].isSymbol(';') && parenDepth == 0 {
				break
			}
			if tokens[cursor].isSymbol('(') {
				parenDepth++
			} else if tokens[cursor].isSymbol(')') {
				parenDepth--
			} else if tokens[cursor].isSymbol('{') && parenDepth == 0 {
				open = cursor
				break
			}
			cursor++
		}
		if open < 0 {
			index++
			continue
		}

		prefix := removeAttributes(tokens[structIndex+1 : open])
		tag := ""
		hasTag := false
		for _, t := range prefix {
			if t.kind == tokenIdent && !isAttributeWord(t.str) {
				tag = t.str
				hasTag = true
			}
		}

		close, err := matchingBrace(tokens, open)
		if err != nil {
			return nil, err
		}
		declarationEnd := close + 1
		for declarationEnd < len(tokens) && !tokens[declarationEnd].isSymbol(';') {
			declarationEnd++
		}
		hasPackedAttribute := false
		for _, t := range tokens[structIndex:open] {
			if t.isIdent("packed") || t.isIdent("PACKED") {
				hasPackedAttribute = true
				break
			}
		}
		if !hasPackedAttribute {
			for _, t := range tokens[close+1 : declarationEnd] {
				if t.isIdent("packed") || t.isIdent("PACKED") {
					hasPackedAttribute = true
					break
				}
			}
		}
		pack := uint64(0)
		hasPack := false
		if hasPackedAttribute {
			pack, hasPack = 1, true
		} else if open < len(packByLine) && packByLine[tokens[open].line] != nil {
			pack, hasPack = *packByLine[tokens[open].line], true
		}

		var name string
		var endIndex int
		if typedef {
			after := declarationEnd
			suffix := removeAttributes(tokens[close+1 : after])
			firstAlias := firstCommaSegment(suffix)
			alias := ""
			found := false
			for _, t := range firstAlias {
				if t.kind == tokenIdent && !isAttributeWord(t.str) {
					alias = t.str
					found = true
					break
				}
			}
			if !found {
				if hasTag {
					alias = tag
					found = true
				} else {
					return nil, fmt.Errorf("typedef 聚合类型缺少名称。")
				}
			}
			name, endIndex = alias, after
		} else {
			if !hasTag {
				return nil, fmt.Errorf("匿名 struct 需要通过 typedef 指定名称。")
			}
			name, endIndex = tag, close
		}

		duplicate := false
		for _, d := range drafts {
			if d.name == name {
				duplicate = true
				break
			}
		}
		if !duplicate {
			drafts = append(drafts, structDraft{
				name:    name,
				tag:     tag,
				hasTag:  hasTag,
				body:    append([]token{}, tokens[open+1:close]...),
				pack:    pack,
				hasPack: hasPack,
				isUnion: isUnion,
			})
		}
		if endIndex+1 > index {
			index = endIndex + 1
		} else {
			index++
		}
	}

	return drafts, nil
}

func firstCommaSegment(tokens []token) []token {
	depth := 0
	for i, t := range tokens {
		if t.isSymbol('[') || t.isSymbol('(') {
			depth++
		} else if t.isSymbol(']') || t.isSymbol(')') {
			depth--
		} else if t.isSymbol(',') && depth == 0 {
			return tokens[:i]
		}
	}
	return tokens
}

func collectTypeAliases(tokens []token, structAliases map[string]string) map[string]typeAlias {
	result := map[string]typeAlias{}
	index := 0
	for index < len(tokens) {
		if !tokens[index].isIdent("typedef") {
			index++
			continue
		}

		start := index + 1
		end := start
		braceDepth := 0
		for end < len(tokens) {
			if tokens[end].isSymbol('{') {
				braceDepth++
			} else if tokens[end].isSymbol('}') {
				braceDepth--
			} else if tokens[end].isSymbol(';') && braceDepth == 0 {
				break
			}
			end++
		}
		clean := removeAttributes(tokens[start:end])
		hasBrace := false
		for _, t := range clean {
			if t.isSymbol('{') {
				hasBrace = true
				break
			}
		}
		if hasBrace {
			if end+1 > index {
				index = end + 1
			} else {
				index++
			}
			continue
		}
		first := firstCommaSegment(clean)
		aliasIndex := declaratorNameIndex(first)
		if aliasIndex < 0 {
			if end+1 > index {
				index = end + 1
			} else {
				index++
			}
			continue
		}
		if first[aliasIndex].kind != tokenIdent {
			if end+1 > index {
				index = end + 1
			} else {
				index++
			}
			continue
		}
		aliasName := first[aliasIndex].str
		pointerDepth := 0
		for _, t := range first[:aliasIndex] {
			if t.isSymbol('*') {
				pointerDepth++
			}
		}
		var words []string
		for _, t := range first[:aliasIndex] {
			if t.kind == tokenIdent && !isQualifier(t.str) && !isAttributeWord(t.str) {
				words = append(words, t.str)
			}
		}
		var target string
		switch {
		case len(words) > 0 && (words[0] == "struct" || words[0] == "union"):
			if len(words) > 1 {
				if canonical, ok := structAliases[words[1]]; ok {
					target = canonical
				} else {
					target = words[1]
				}
			}
		case len(words) > 0 && words[0] == "enum":
			target = "enum"
		default:
			target = strings.Join(words, " ")
		}
		if target != "" && target != aliasName {
			result[aliasName] = typeAlias{target: target, pointerDepth: pointerDepth}
		}
		if end+1 > index {
			index = end + 1
		} else {
			index++
		}
	}
	return result
}

func joinLineContinuations(source string) string {
	return strings.ReplaceAll(strings.ReplaceAll(source, "\\\r\n", " "), "\\\n", " ")
}

// ---- conditional compilation ----

type conditionalFrame struct {
	parentActive bool
	branchTaken  bool
}

func filterConditionalBlocks(source string) (string, error) {
	var output strings.Builder
	var frames []conditionalFrame
	active := true
	defined := map[string]bool{"_WIN32": true, "_WIN64": true}
	objectMacros := map[string]string{"_WIN32": "1", "_WIN64": "1"}

	for lineIndex, line := range strings.Split(source, "\n") {
		trimmed := strings.TrimLeftFunc(line, isSpace)
		var keyword, expression string
		hasDirective := false
		if rest, ok := cutPrefix(trimmed, "#"); ok {
			rest = trimLeftSpaces(rest)
			end := strings.IndexFunc(rest, unicode.IsSpace)
			if end < 0 {
				end = len(rest)
			}
			keyword, expression, hasDirective = rest[:end], strings.TrimSpace(rest[end:]), true
		}
		keepLine := active

		if hasDirective {
			switch keyword {
			case "if":
				condition := active && evaluatePreprocessorCondition(expression, defined, objectMacros)
				frames = append(frames, conditionalFrame{parentActive: active, branchTaken: condition})
				active = condition
				keepLine = false
			case "ifdef":
				name := firstIdentifier(expression)
				condition := active && name != "" && defined[name]
				frames = append(frames, conditionalFrame{parentActive: active, branchTaken: condition})
				active = condition
				keepLine = false
			case "ifndef":
				name := firstIdentifier(expression)
				condition := active && !(name != "" && defined[name])
				frames = append(frames, conditionalFrame{parentActive: active, branchTaken: condition})
				active = condition
				keepLine = false
			case "elif":
				if len(frames) == 0 {
					return "", fmt.Errorf("第 %d 行出现了多余的 #elif。", lineIndex+1)
				}
				frame := &frames[len(frames)-1]
				condition := frame.parentActive && !frame.branchTaken && evaluatePreprocessorCondition(expression, defined, objectMacros)
				frame.branchTaken = frame.branchTaken || condition
				active = condition
				keepLine = false
			case "else":
				if len(frames) == 0 {
					return "", fmt.Errorf("第 %d 行出现了多余的 #else。", lineIndex+1)
				}
				frame := &frames[len(frames)-1]
				active = frame.parentActive && !frame.branchTaken
				frame.branchTaken = true
				keepLine = false
			case "endif":
				if len(frames) == 0 {
					return "", fmt.Errorf("第 %d 行出现了多余的 #endif。", lineIndex+1)
				}
				frame := frames[len(frames)-1]
				frames = frames[:len(frames)-1]
				active = frame.parentActive
				keepLine = false
			case "define":
				if active {
					if name, functionLike, replacement, ok := macroSignature(expression); ok {
						defined[name] = true
						if functionLike {
							delete(objectMacros, name)
						} else {
							objectMacros[name] = replacement
						}
					}
				}
			case "undef":
				if active {
					if name := firstIdentifier(expression); name != "" {
						delete(defined, name)
						delete(objectMacros, name)
					}
				}
				keepLine = false
			}
		}

		if keepLine {
			output.WriteString(line)
		}
		output.WriteByte('\n')
	}

	if len(frames) != 0 {
		return "", fmt.Errorf("存在未闭合的条件编译指令，请检查 #if/#ifdef 与 #endif。")
	}
	return output.String(), nil
}

func firstIdentifier(value string) string {
	length := 0
	for _, ch := range value {
		if isASCIIAlnum(ch) || ch == '_' {
			length++
		} else {
			break
		}
	}
	if length == 0 {
		return ""
	}
	return value[:length]
}

func macroSignature(definition string) (string, bool, string, bool) {
	name := firstIdentifier(definition)
	if name == "" {
		return "", false, "", false
	}
	suffix := definition[len(name):]
	if strings.HasPrefix(suffix, "(") {
		close := matchingMacroParenthesis(suffix)
		if close < 0 {
			return name, false, "", false
		}
		return name, true, strings.TrimSpace(suffix[close+1:]), true
	}
	return name, false, strings.TrimSpace(suffix), true
}

func evaluatePreprocessorCondition(expression string, defined map[string]bool, objectMacros map[string]string) bool {
	values := map[string]uint64{}
	for i := 0; i < 32; i++ {
		changed := false
		for name, replacement := range objectMacros {
			if _, done := values[name]; done {
				continue
			}
			if tokens, err := tokenize(replacement); err == nil {
				if value, ok := evaluateConstant(tokens, values); ok {
					values[name] = value
					changed = true
				}
			}
		}
		if !changed {
			break
		}
	}

	tokens, err := tokenize(expression)
	if err != nil {
		return false
	}
	var normalized []token
	index := 0
	for index < len(tokens) {
		if tokens[index].isIdent("defined") {
			parenthesized := index+1 < len(tokens) && tokens[index+1].isSymbol('(')
			nameIndex := index + 1
			if parenthesized {
				nameIndex = index + 2
			}
			if nameIndex >= len(tokens) || tokens[nameIndex].kind != tokenIdent {
				return false
			}
			name := tokens[nameIndex].str
			var num uint64
			if defined[name] {
				num = 1
			}
			normalized = append(normalized, token{kind: tokenNumber, num: num, line: tokens[index].line})
			index = nameIndex + 1
			if parenthesized && index < len(tokens) && tokens[index].isSymbol(')') {
				index++
			}
		} else if tokens[index].kind == tokenIdent {
			normalized = append(normalized, token{kind: tokenNumber, num: values[tokens[index].str], line: tokens[index].line})
			index++
		} else {
			normalized = append(normalized, tokens[index])
			index++
		}
	}

	value, ok := evaluateConstant(normalized, map[string]uint64{})
	return ok && value != 0
}

func collectMacroDefinitions(source string) (map[string]macroDefinition, error) {
	definitions := map[string]macroDefinition{}

	for lineIndex, line := range strings.Split(source, "\n") {
		trimmed := strings.TrimLeftFunc(line, isSpace)
		rest, ok := cutPrefix(trimmed, "#")
		if !ok {
			continue
		}
		rest, ok = cutPrefix(trimLeftSpaces(rest), "define")
		if !ok {
			continue
		}
		rest = trimLeftSpaces(rest)
		nameLen := 0
		for _, ch := range rest {
			if isASCIIAlnum(ch) || ch == '_' {
				nameLen++
			} else {
				break
			}
		}
		if nameLen == 0 {
			continue
		}

		name := rest[:nameLen]
		suffix := rest[nameLen:]
		var params []string
		hasParams := false
		replacement := strings.TrimSpace(suffix)
		if strings.HasPrefix(suffix, "(") {
			close := matchingMacroParenthesis(suffix)
			if close < 0 {
				return nil, fmt.Errorf("第 %d 行的宏 `%s` 参数列表未闭合。", lineIndex+1, name)
			}
			rawParams := suffix[1:close]
			valid := true
			for _, param := range strings.Split(rawParams, ",") {
				param = strings.TrimSpace(param)
				if param == "" {
					continue
				}
				if !isCIdentifier(param) {
					valid = false
					break
				}
				params = append(params, param)
			}
			if !valid {
				continue
			}
			hasParams = true
			replacement = strings.TrimSpace(suffix[close+1:])
		}

		replacementTokens, err := tokenize(replacement)
		if err != nil {
			return nil, err
		}
		for i := range replacementTokens {
			replacementTokens[i].line = lineIndex + 1
		}
		definitions[name] = macroDefinition{params: params, hasParams: hasParams, replacement: replacementTokens}
	}

	return definitions, nil
}

func matchingMacroParenthesis(value string) int {
	depth := 0
	for index, ch := range value {
		if ch == '(' {
			depth++
		} else if ch == ')' {
			depth--
			if depth == 0 {
				return index
			}
		}
	}
	return -1
}

func isCIdentifier(value string) bool {
	if value == "" {
		return false
	}
	for i, ch := range value {
		if i == 0 {
			if !isASCIIAlpha(ch) && ch != '_' {
				return false
			}
		} else if !isASCIIAlnum(ch) && ch != '_' {
			return false
		}
	}
	return true
}

// ---- macro expansion ----

func expandMacroTokens(tokens []token, definitions map[string]macroDefinition) ([]token, error) {
	active := map[string]bool{}
	return expandMacroTokensInner(tokens, definitions, active, 0)
}

func expandMacroTokensInner(tokens []token, definitions map[string]macroDefinition, active map[string]bool, depth int) ([]token, error) {
	if depth >= maxMacroExpansionDepth {
		return nil, fmt.Errorf("宏展开层级过深，请检查宏是否存在循环引用。")
	}

	var result []token
	index := 0
	for index < len(tokens) {
		current := tokens[index]
		if current.kind != tokenIdent {
			result = append(result, current)
			index++
			continue
		}
		definition, ok := definitions[current.str]
		if !ok {
			result = append(result, current)
			index++
			continue
		}
		if active[current.str] {
			result = append(result, current)
			index++
			continue
		}

		var replacement []token
		consumed := 1
		if definition.hasParams {
			if !(index+1 < len(tokens) && tokens[index+1].isSymbol('(')) {
				result = append(result, current)
				index++
				continue
			}
			arguments, closeIndex, ok := macroArguments(tokens, index+1)
			if !ok {
				result = append(result, current)
				index++
				continue
			}
			if len(arguments) != len(definition.params) {
				result = append(result, current)
				index++
				continue
			}
			substitutions := map[string][]token{}
			for i, param := range definition.params {
				expanded, err := expandMacroTokensInner(arguments[i], definitions, active, depth+1)
				if err != nil {
					return nil, err
				}
				substitutions[param] = expanded
			}
			for _, replacementToken := range definition.replacement {
				if replacementToken.kind == tokenIdent {
					if value, ok := substitutions[replacementToken.str]; ok {
						replacement = append(replacement, value...)
						continue
					}
				}
				replacement = append(replacement, replacementToken)
			}
			consumed = closeIndex - index + 1
		} else {
			replacement = append([]token{}, definition.replacement...)
		}

		for i := range replacement {
			replacement[i].line = current.line
		}
		active[current.str] = true
		expanded, err := expandMacroTokensInner(replacement, definitions, active, depth+1)
		delete(active, current.str)
		if err != nil {
			return nil, err
		}
		result = append(result, expanded...)
		if len(result) > maxMacroExpansionTokens {
			return nil, fmt.Errorf("宏展开后超过 %d 个标记，请缩小宏展开规模。", maxMacroExpansionTokens)
		}
		index += consumed
	}
	return result, nil
}

func macroArguments(tokens []token, openIndex int) ([][]token, int, bool) {
	if openIndex >= len(tokens) || !tokens[openIndex].isSymbol('(') {
		return nil, 0, false
	}
	var arguments [][]token
	var current []token
	depth := 1
	index := openIndex + 1
	for index < len(tokens) {
		switch {
		case tokens[index].isSymbol('('):
			depth++
			current = append(current, tokens[index])
		case tokens[index].isSymbol(')'):
			depth--
			if depth == 0 {
				if len(current) > 0 || len(arguments) > 0 {
					arguments = append(arguments, current)
				}
				return arguments, index, true
			}
			current = append(current, tokens[index])
		case tokens[index].isSymbol(',') && depth == 1:
			arguments = append(arguments, current)
			current = nil
		default:
			current = append(current, tokens[index])
		}
		index++
	}
	return nil, 0, false
}

func collectIntegerMacros(source string) map[string]uint64 {
	joined := strings.ReplaceAll(strings.ReplaceAll(source, "\\\r\n", " "), "\\\n", " ")
	type pendingMacro struct {
		name       string
		expression string
	}
	var pending []pendingMacro
	for _, line := range strings.Split(joined, "\n") {
		trimmed := strings.TrimLeftFunc(line, isSpace)
		rest, ok := cutPrefix(trimmed, "#")
		if !ok {
			continue
		}
		rest, ok = cutPrefix(trimLeftSpaces(rest), "define")
		if !ok {
			continue
		}
		rest = trimLeftSpaces(rest)
		nameLen := 0
		for _, ch := range rest {
			if isASCIIAlnum(ch) || ch == '_' {
				nameLen++
			} else {
				break
			}
		}
		if nameLen == 0 || strings.HasPrefix(rest[nameLen:], "(") {
			continue
		}
		name := rest[:nameLen]
		expression := rest[nameLen:]
		if idx := strings.Index(expression, "//"); idx >= 0 {
			expression = expression[:idx]
		}
		if idx := strings.Index(expression, "/*"); idx >= 0 {
			expression = expression[:idx]
		}
		expression = strings.TrimSpace(expression)
		if expression != "" {
			pending = append(pending, pendingMacro{name: name, expression: expression})
		}
	}

	values := map[string]uint64{}
	for i := 0; i < 32; i++ {
		changed := false
		for _, macro := range pending {
			if _, done := values[macro.name]; done {
				continue
			}
			if tokens, err := tokenize(macro.expression); err == nil {
				if value, ok := evaluateConstant(tokens, values); ok {
					values[macro.name] = value
					changed = true
				}
			}
		}
		if !changed {
			break
		}
	}
	return values
}

// ---- constant expression evaluation ----

type constantParser struct {
	tokens []token
	index  int
	values map[string]uint64
}

func evaluateConstant(tokens []token, values map[string]uint64) (uint64, bool) {
	var code []token
	for _, t := range tokens {
		if !t.isComment() {
			code = append(code, t)
		}
	}
	parser := &constantParser{tokens: code, values: values}
	value, ok := parser.parseLogicalOr()
	if !ok || parser.index != len(code) {
		return 0, false
	}
	return value, true
}

func (p *constantParser) peek() *token {
	if p.index < len(p.tokens) {
		return &p.tokens[p.index]
	}
	return nil
}

func (p *constantParser) takeSymbol(symbol rune) bool {
	if t := p.peek(); t != nil && t.isSymbol(symbol) {
		p.index++
		return true
	}
	return false
}

func (p *constantParser) takePair(first, second rune) bool {
	if p.index+1 < len(p.tokens) && p.tokens[p.index].isSymbol(first) && p.tokens[p.index+1].isSymbol(second) {
		p.index += 2
		return true
	}
	return false
}

func (p *constantParser) takeSingle(symbol rune) bool {
	if t := p.peek(); t == nil || !t.isSymbol(symbol) {
		return false
	}
	if p.index+1 < len(p.tokens) {
		next := &p.tokens[p.index+1]
		if next.isSymbol(symbol) || next.isSymbol('=') {
			return false
		}
	}
	p.index++
	return true
}

func (p *constantParser) parseLogicalOr() (uint64, bool) {
	value, ok := p.parseLogicalAnd()
	if !ok {
		return 0, false
	}
	for p.takePair('|', '|') {
		right, ok := p.parseLogicalAnd()
		if !ok {
			return 0, false
		}
		if value != 0 || right != 0 {
			value = 1
		} else {
			value = 0
		}
	}
	return value, true
}

func (p *constantParser) parseLogicalAnd() (uint64, bool) {
	value, ok := p.parseBitOr()
	if !ok {
		return 0, false
	}
	for p.takePair('&', '&') {
		right, ok := p.parseBitOr()
		if !ok {
			return 0, false
		}
		if value != 0 && right != 0 {
			value = 1
		} else {
			value = 0
		}
	}
	return value, true
}

func (p *constantParser) parseBitOr() (uint64, bool) {
	value, ok := p.parseXor()
	if !ok {
		return 0, false
	}
	for p.takeSingle('|') {
		right, ok := p.parseXor()
		if !ok {
			return 0, false
		}
		value |= right
	}
	return value, true
}

func (p *constantParser) parseXor() (uint64, bool) {
	value, ok := p.parseBitAnd()
	if !ok {
		return 0, false
	}
	for p.takeSymbol('^') {
		right, ok := p.parseBitAnd()
		if !ok {
			return 0, false
		}
		value ^= right
	}
	return value, true
}

func (p *constantParser) parseBitAnd() (uint64, bool) {
	value, ok := p.parseEquality()
	if !ok {
		return 0, false
	}
	for p.takeSingle('&') {
		right, ok := p.parseEquality()
		if !ok {
			return 0, false
		}
		value &= right
	}
	return value, true
}

func (p *constantParser) parseEquality() (uint64, bool) {
	value, ok := p.parseRelational()
	if !ok {
		return 0, false
	}
	for {
		if p.takePair('=', '=') {
			right, ok := p.parseRelational()
			if !ok {
				return 0, false
			}
			if value == right {
				value = 1
			} else {
				value = 0
			}
		} else if p.takePair('!', '=') {
			right, ok := p.parseRelational()
			if !ok {
				return 0, false
			}
			if value != right {
				value = 1
			} else {
				value = 0
			}
		} else {
			break
		}
	}
	return value, true
}

func (p *constantParser) parseRelational() (uint64, bool) {
	value, ok := p.parseShift()
	if !ok {
		return 0, false
	}
	for {
		if p.takePair('<', '=') {
			right, ok := p.parseShift()
			if !ok {
				return 0, false
			}
			if value <= right {
				value = 1
			} else {
				value = 0
			}
		} else if p.takePair('>', '=') {
			right, ok := p.parseShift()
			if !ok {
				return 0, false
			}
			if value >= right {
				value = 1
			} else {
				value = 0
			}
		} else if p.takeSingle('<') {
			right, ok := p.parseShift()
			if !ok {
				return 0, false
			}
			if value < right {
				value = 1
			} else {
				value = 0
			}
		} else if p.takeSingle('>') {
			right, ok := p.parseShift()
			if !ok {
				return 0, false
			}
			if value > right {
				value = 1
			} else {
				value = 0
			}
		} else {
			break
		}
	}
	return value, true
}

func (p *constantParser) parseShift() (uint64, bool) {
	value, ok := p.parseAdd()
	if !ok {
		return 0, false
	}
	for {
		if p.takePair('<', '<') {
			right, ok := p.parseAdd()
			if !ok {
				return 0, false
			}
			if right >= 64 {
				return 0, false
			}
			value <<= right
		} else if p.takePair('>', '>') {
			right, ok := p.parseAdd()
			if !ok {
				return 0, false
			}
			if right >= 64 {
				return 0, false
			}
			value >>= right
		} else {
			break
		}
	}
	return value, true
}

func (p *constantParser) parseAdd() (uint64, bool) {
	value, ok := p.parseMul()
	if !ok {
		return 0, false
	}
	for {
		if p.takeSymbol('+') {
			right, ok := p.parseMul()
			if !ok {
				return 0, false
			}
			next := value + right
			if next < value {
				return 0, false
			}
			value = next
		} else if p.takeSymbol('-') {
			right, ok := p.parseMul()
			if !ok {
				return 0, false
			}
			if right > value {
				return 0, false
			}
			value -= right
		} else {
			break
		}
	}
	return value, true
}

func (p *constantParser) parseMul() (uint64, bool) {
	value, ok := p.parseUnary()
	if !ok {
		return 0, false
	}
	for {
		if p.takeSymbol('*') {
			right, ok := p.parseUnary()
			if !ok {
				return 0, false
			}
			if value != 0 && right > (1<<64-1)/value {
				return 0, false
			}
			value *= right
		} else if p.takeSymbol('/') {
			right, ok := p.parseUnary()
			if !ok || right == 0 {
				return 0, false
			}
			value /= right
		} else if p.takeSymbol('%') {
			right, ok := p.parseUnary()
			if !ok || right == 0 {
				return 0, false
			}
			value %= right
		} else {
			break
		}
	}
	return value, true
}

func (p *constantParser) parseUnary() (uint64, bool) {
	if p.takeSymbol('+') {
		return p.parseUnary()
	}
	if p.takeSymbol('-') {
		value, ok := p.parseUnary()
		if !ok {
			return 0, false
		}
		return 0 - value, true
	}
	if p.takeSymbol('~') {
		value, ok := p.parseUnary()
		if !ok {
			return 0, false
		}
		return ^value, true
	}
	if p.takeSymbol('!') {
		value, ok := p.parseUnary()
		if !ok {
			return 0, false
		}
		if value == 0 {
			return 1, true
		}
		return 0, true
	}
	return p.parsePrimary()
}

func (p *constantParser) parsePrimary() (uint64, bool) {
	t := p.peek()
	if t == nil {
		return 0, false
	}
	switch t.kind {
	case tokenNumber:
		p.index++
		return t.num, true
	case tokenIdent:
		p.index++
		value, ok := p.values[t.str]
		if !ok {
			return 0, false
		}
		return value, true
	case tokenSymbol:
		if t.sym == '(' {
			p.index++
			value, ok := p.parseLogicalOr()
			if !ok {
				return 0, false
			}
			if !p.takeSymbol(')') {
				return 0, false
			}
			return value, true
		}
	}
	return 0, false
}

func matchingBrace(tokens []token, open int) (int, error) {
	depth := 0
	for index := open; index < len(tokens); index++ {
		if tokens[index].isSymbol('{') {
			depth++
		} else if tokens[index].isSymbol('}') {
			if depth > 0 {
				depth--
			}
			if depth == 0 {
				return index, nil
			}
		}
	}
	return 0, fmt.Errorf("存在未闭合的结构体大括号。")
}

// ---- field parsing ----

func parseFields(body []token, aliases map[string]string, typeAliases map[string]typeAlias, macroValues map[string]uint64) []fieldDef {
	var fields []fieldDef
	start := 0
	braceDepth := 0
	parenDepth := 0

	for index, current := range body {
		if current.isSymbol('{') {
			braceDepth++
		} else if current.isSymbol('}') {
			braceDepth--
		} else if current.isSymbol('(') {
			parenDepth++
		} else if current.isSymbol(')') {
			parenDepth--
		} else if current.isSymbol(';') && braceDepth == 0 && parenDepth == 0 {
			declaration := append([]token{}, body[start:index]...)
			nextStart := index + 1
			for nextStart < len(body) {
				next := body[nextStart]
				if next.isComment() && next.line == current.line {
					declaration = append(declaration, next)
					nextStart++
				} else {
					break
				}
			}
			parseDeclaration(declaration, aliases, typeAliases, macroValues, &fields)
			start = nextStart
		}
	}
	return fields
}

func parseDeclaration(declaration []token, aliases map[string]string, typeAliases map[string]typeAlias, macroValues map[string]uint64, fields *[]fieldDef) {
	var comments []string
	for _, t := range declaration {
		if t.isComment() && t.str != "" {
			comments = append(comments, t.str)
		}
	}
	comment := strings.Join(comments, " ")

	var code []token
	for _, t := range removeAttributes(declaration) {
		if !t.isComment() {
			code = append(code, t)
		}
	}

	if len(code) == 0 {
		return
	}
	for _, t := range code {
		if t.isSymbol('{') || t.isIdent("static_assert") {
			return
		}
	}

	segments := splitOnCommas(code)
	if len(segments) == 0 {
		return
	}
	first := segments[0]
	firstNameIndex := declaratorNameIndex(first)
	if firstNameIndex < 0 {
		return
	}
	typeName, firstPointerDepth := normalizeType(first[:firstNameIndex], aliases, typeAliases)
	if typeName == "" {
		return
	}

	for segmentIndex, segment := range segments {
		nameIndex := declaratorNameIndex(segment)
		if nameIndex < 0 {
			continue
		}
		if segment[nameIndex].kind != tokenIdent {
			continue
		}
		name := segment[nameIndex].str
		pointerDepth := firstPointerDepth
		if segmentIndex != 0 {
			pointerDepth = 0
			for _, t := range segment[:nameIndex] {
				if t.isSymbol('*') {
					pointerDepth++
				}
			}
		}
		arrayLen := arrayLength(segment[nameIndex+1:], macroValues)
		bitWidth, hasBitWidth := bitWidthOf(segment[nameIndex+1:], macroValues)

		*fields = append(*fields, fieldDef{
			name:         name,
			typeName:     typeName,
			pointerDepth: pointerDepth,
			arrayLen:     arrayLen,
			bitWidth:     bitWidth,
			hasBitWidth:  hasBitWidth,
			comment:      comment,
		})
	}
}

func splitOnCommas(tokens []token) [][]token {
	var result [][]token
	start := 0
	bracketDepth := 0
	parenDepth := 0
	for index, current := range tokens {
		if current.isSymbol('[') {
			bracketDepth++
		} else if current.isSymbol(']') {
			bracketDepth--
		} else if current.isSymbol('(') {
			parenDepth++
		} else if current.isSymbol(')') {
			parenDepth--
		} else if current.isSymbol(',') && bracketDepth == 0 && parenDepth == 0 {
			result = append(result, tokens[start:index])
			start = index + 1
		}
	}
	result = append(result, tokens[start:])
	return result
}

func declaratorNameIndex(tokens []token) int {
	end := len(tokens)
	for i, t := range tokens {
		if t.isSymbol('[') || t.isSymbol(':') {
			end = i
			break
		}
	}
	for i := end - 1; i >= 0; i-- {
		t := tokens[i]
		if t.kind == tokenIdent && !isTypeKeyword(t.str) && !strings.HasPrefix(t.str, "__") {
			return i
		}
	}
	return -1
}

func normalizeType(tokens []token, aliases map[string]string, typeAliases map[string]typeAlias) (string, int) {
	pointerDepth := 0
	for _, t := range tokens {
		if t.isSymbol('*') {
			pointerDepth++
		}
	}
	var words []string
	for _, t := range tokens {
		if t.kind == tokenIdent && !isQualifier(t.str) && !isAttributeWord(t.str) && !isAnnotationWord(t.str) {
			words = append(words, t.str)
		}
	}

	if len(words) > 0 && (words[0] == "struct" || words[0] == "union") {
		if len(words) > 1 {
			if canonical, ok := aliases[words[1]]; ok {
				return canonical, pointerDepth
			}
			return words[1], pointerDepth
		}
	}
	if len(words) > 0 && words[0] == "enum" {
		return "enum", pointerDepth
	}

	for _, word := range words {
		if _, ok := aliases[word]; ok {
			if canonical, ok := aliases[word]; ok {
				return canonical, pointerDepth
			}
		}
	}

	typeName := strings.Join(words, " ")
	for i := len(words) - 1; i >= 0; i-- {
		if _, ok := typeAliases[words[i]]; ok {
			typeName = words[i]
			break
		}
	}
	visited := map[string]bool{}
	for {
		alias, ok := typeAliases[typeName]
		if !ok {
			break
		}
		if visited[typeName] {
			break
		}
		visited[typeName] = true
		pointerDepth += alias.pointerDepth
		typeName = alias.target
	}
	if canonical, ok := aliases[typeName]; ok {
		typeName = canonical
	}
	return typeName, pointerDepth
}

func isAnnotationWord(value string) bool {
	if strings.HasPrefix(value, "_") && strings.HasSuffix(value, "_") {
		return true
	}
	switch value {
	case "IN", "OUT", "INOUT", "OPTIONAL", "FAR", "NEAR":
		return true
	}
	return false
}

func isQualifier(value string) bool {
	switch value {
	case "const", "volatile", "static", "extern", "register", "auto", "restrict", "_Atomic", "typedef":
		return true
	}
	return false
}

func isTypeKeyword(value string) bool {
	if isQualifier(value) {
		return true
	}
	switch value {
	case "struct", "enum", "union", "signed", "unsigned", "short", "long", "int", "char", "float", "double", "void", "_Bool", "bool":
		return true
	}
	return false
}

func arrayLength(tokens []token, macroValues map[string]uint64) uint64 {
	length := uint64(1)
	found := false
	index := 0
	for index < len(tokens) {
		if tokens[index].isSymbol('[') {
			found = true
			close := index + 1
			depth := 1
			for close < len(tokens) && depth > 0 {
				if tokens[close].isSymbol('[') {
					depth++
				} else if tokens[close].isSymbol(']') {
					depth--
				}
				if depth > 0 {
					close++
				}
			}
			dimension := uint64(1)
			if value, ok := evaluateConstant(tokens[index+1:close], macroValues); ok {
				dimension = value
			}
			if dimension < 1 {
				dimension = 1
			}
			next := length * dimension
			if next < length {
				next = 1 << 63 // saturating
			}
			length = next
		}
		index++
	}
	if found {
		return length
	}
	return 1
}

func bitWidthOf(tokens []token, macroValues map[string]uint64) (uint64, bool) {
	for index, t := range tokens {
		if t.isSymbol(':') {
			if value, ok := evaluateConstant(tokens[index+1:], macroValues); ok {
				return value, true
			}
			return 0, false
		}
	}
	return 0, false
}

func alignUp(value, alignment uint64) uint64 {
	if alignment <= 1 {
		return value
	}
	return (value + alignment - 1) / alignment * alignment
}

// ---- layout ----

func structLayout(name string, definitions map[string]*structDef, aliases map[string]string, cache map[string]typeLayout, visiting map[string]bool, warnings *[]string) (typeLayout, error) {
	canonical := name
	if mapped, ok := aliases[name]; ok {
		canonical = mapped
	}
	if layout, ok := cache[canonical]; ok {
		return layout, nil
	}
	if visiting[canonical] {
		return typeLayout{}, fmt.Errorf("结构体 `%s` 存在按值递归引用；请改为指针或检查定义。", canonical)
	}
	visiting[canonical] = true

	definition, ok := definitions[canonical]
	if !ok {
		return typeLayout{}, fmt.Errorf("缺少结构体 `%s` 的定义。", canonical)
	}
	_, size, maxAlign, err := layoutFields(definition, definitions, aliases, cache, visiting, warnings)
	if err != nil {
		return typeLayout{}, err
	}
	delete(visiting, canonical)

	layout := typeLayout{
		size:        size,
		align:       maxAlign,
		displayName: canonical,
		nestedName:  canonical,
		hasNested:   true,
	}
	cache[canonical] = layout
	return layout, nil
}

func layoutFields(definition *structDef, definitions map[string]*structDef, aliases map[string]string, cache map[string]typeLayout, visiting map[string]bool, warnings *[]string) ([]fieldPlacement, uint64, uint64, error) {
	placements := make([]fieldPlacement, 0, len(definition.fields))

	if definition.isUnion {
		unionSize := uint64(0)
		unionAlign := uint64(1)
		for _, field := range definition.fields {
			layout, err := fieldLayout(field, definitions, aliases, cache, visiting, warnings)
			if err != nil {
				return nil, 0, 0, err
			}
			effectiveAlign := layout.align
			if definition.hasPack && layout.align > definition.pack {
				effectiveAlign = definition.pack
			}
			if effectiveAlign < 1 {
				effectiveAlign = 1
			}
			if layout.size > unionSize {
				unionSize = layout.size
			}
			if effectiveAlign > unionAlign {
				unionAlign = effectiveAlign
			}
			placements = append(placements, fieldPlacement{offset: 0, layout: layout})
		}
		return placements, alignUp(unionSize, unionAlign), unionAlign, nil
	}

	offset := uint64(0)
	maxAlign := uint64(1)
	// start offset, storage size, bits already used
	bitUnitStart := uint64(0)
	bitUnitSize := uint64(0)
	bitUnitUsed := uint64(0)
	hasBitUnit := false

	for _, field := range definition.fields {
		layout, err := fieldLayout(field, definitions, aliases, cache, visiting, warnings)
		if err != nil {
			return nil, 0, 0, err
		}
		effectiveAlign := layout.align
		if definition.hasPack && layout.align > definition.pack {
			effectiveAlign = definition.pack
		}
		if effectiveAlign < 1 {
			effectiveAlign = 1
		}
		if effectiveAlign > maxAlign {
			maxAlign = effectiveAlign
		}

		var fieldOffset uint64
		if field.hasBitWidth {
			width := field.bitWidth
			storageBits := saturatingMul(layout.size, 8)
			reuse := hasBitUnit && bitUnitSize == layout.size && width > 0 && saturatingAdd(bitUnitUsed, width) <= storageBits
			if reuse {
				bitUnitUsed += width
				fieldOffset = bitUnitStart
			} else {
				offset = alignUp(offset, effectiveAlign)
				start := offset
				if width == 0 {
					hasBitUnit = false
				} else {
					offset = saturatingAdd(offset, layout.size)
					bitUnitStart, bitUnitSize, bitUnitUsed, hasBitUnit = start, layout.size, minUint(width, storageBits), true
				}
				fieldOffset = start
			}
		} else {
			hasBitUnit = false
			offset = alignUp(offset, effectiveAlign)
			start := offset
			offset = saturatingAdd(offset, layout.size)
			fieldOffset = start
		}
		placements = append(placements, fieldPlacement{offset: fieldOffset, layout: layout})
	}

	structAlign := maxAlign
	if structAlign < 1 {
		structAlign = 1
	}
	return placements, alignUp(offset, structAlign), structAlign, nil
}

func fieldLayout(field fieldDef, definitions map[string]*structDef, aliases map[string]string, cache map[string]typeLayout, visiting map[string]bool, warnings *[]string) (typeLayout, error) {
	var layout typeLayout
	var err error
	if field.pointerDepth > 0 {
		layout = typeLayout{size: 8, align: 8, displayName: "POINTER"}
	} else if canonical, ok := aliases[field.typeName]; ok {
		layout, err = structLayout(canonical, definitions, aliases, cache, visiting, warnings)
		if err != nil {
			return typeLayout{}, err
		}
	} else {
		layout = primitiveLayout(field.typeName, warnings)
	}

	arrayLen := field.arrayLen
	if arrayLen < 1 {
		arrayLen = 1
	}
	layout.size = saturatingMul(layout.size, arrayLen)
	if field.arrayLen > 1 {
		layout.displayName = fmt.Sprintf("%s[%d]", layout.displayName, field.arrayLen)
	}
	return layout, nil
}

func primitiveLayout(typeName string, warnings *[]string) typeLayout {
	normalized := strings.Join(strings.Fields(strings.ToLower(typeName)), " ")

	var size uint64
	var display string
	switch normalized {
	case "bool", "_bool":
		size, display = 1, "BOOL"
	case "char", "signed char", "int8_t":
		size, display = 1, "INT8"
	case "unsigned char", "uint8_t", "byte":
		size, display = 1, "UINT8"
	case "short", "short int", "signed short", "signed short int", "int16_t":
		size, display = 2, "INT16"
	case "unsigned short", "unsigned short int", "uint16_t":
		size, display = 2, "UINT16"
	case "int", "signed", "signed int", "int32_t", "enum":
		size, display = 4, "INT"
	case "unsigned", "unsigned int", "uint32_t":
		size, display = 4, "UINT"
	case "long", "long int", "signed long", "signed long int":
		size, display = 4, "LONG"
	case "unsigned long", "unsigned long int":
		size, display = 4, "ULONG"
	case "long long", "long long int", "signed long long", "int64_t":
		size, display = 8, "INT64"
	case "unsigned long long", "unsigned long long int", "uint64_t":
		size, display = 8, "UINT64"
	case "float":
		size, display = 4, "FLOAT"
	case "double", "long double":
		size, display = 8, "DOUBLE"
	case "size_t", "uintptr_t", "intptr_t", "ptrdiff_t":
		size, display = 8, "UINT64"
	default:
		var digits strings.Builder
		for _, ch := range normalized {
			if isDigit(ch) {
				digits.WriteRune(ch)
			}
		}
		var size uint64 = 4
		switch digits.String() {
		case "8":
			size = 1
		case "16":
			size = 2
		case "32":
			size = 4
		case "64":
			size = 8
		}
		*warnings = append(*warnings, fmt.Sprintf("类型 `%s` 未知，暂按 %d 位整数计算。", typeName, size*8))
		display := fmt.Sprintf("INT%d", size*8)
		if strings.HasPrefix(normalized, "u") || strings.Contains(normalized, "uint") {
			display = fmt.Sprintf("UINT%d", size*8)
		}
		return typeLayout{size: size, align: minUint(size, 8), displayName: display}
	}

	return typeLayout{size: size, align: minUint(size, 8), displayName: display}
}

func saturatingMul(a, b uint64) uint64 {
	if a == 0 || b == 0 {
		return 0
	}
	if a > (1<<64-1)/b {
		return 1 << 63
	}
	return a * b
}

func saturatingAdd(a, b uint64) uint64 {
	next := a + b
	if next < a {
		return 1 << 63
	}
	return next
}

func minUint(a, b uint64) uint64 {
	if a < b {
		return a
	}
	return b
}

// ---- flattening ----

func flattenStruct(structName, prefix string, baseOffset uint64, definitions map[string]*structDef, aliases map[string]string, cache map[string]typeLayout, warnings *[]string, rows *[]FieldRow) error {
	definition, ok := definitions[structName]
	if !ok {
		return fmt.Errorf("缺少结构体 `%s` 的定义。", structName)
	}
	visiting := map[string]bool{}
	placements, _, _, err := layoutFields(definition, definitions, aliases, cache, visiting, warnings)
	if err != nil {
		return err
	}

	for i, field := range definition.fields {
		placement := placements[i]
		layout := placement.layout
		fieldOffset := baseOffset + placement.offset
		path := field.name
		if prefix != "" {
			path = prefix + "." + field.name
		}

		if layout.hasNested {
			elementLayout, ok := cache[layout.nestedName]
			if !ok {
				return fmt.Errorf("无法计算 `%s` 的布局。", layout.nestedName)
			}
			arrayLen := field.arrayLen
			if arrayLen < 1 {
				arrayLen = 1
			}
			for arrayIndex := uint64(0); arrayIndex < arrayLen; arrayIndex++ {
				nestedPath := path
				if field.arrayLen > 1 {
					nestedPath = fmt.Sprintf("%s[%d]", path, arrayIndex)
				}
				if err := flattenStruct(layout.nestedName, nestedPath, fieldOffset+arrayIndex*elementLayout.size, definitions, aliases, cache, warnings, rows); err != nil {
					return err
				}
			}
		} else {
			if len(*rows) >= maxExpandedRows {
				return fmt.Errorf("展开后的字段超过 %d 行，请缩小数组或结构体规模。", maxExpandedRows)
			}
			bitSize := saturatingMul(layout.size, 8)
			if field.hasBitWidth {
				bitSize = field.bitWidth
			}
			groupKey := prefix
			if prefix == "" {
				groupKey = "root:" + structName
			}
			*rows = append(*rows, FieldRow{
				Status:              "Initial",
				ParameterName:       field.name,
				SystemParameterName: field.name,
				DataType:            layout.displayName,
				DataSize:            bitSize,
				InIOBuffer:          "Yes",
				IOBufferOffset:      fieldOffset,
				Comments:            field.comment,
				GroupKey:            groupKey,
			})
		}
	}
	return nil
}
