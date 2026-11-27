# StructSheet

一个使用 Go + MyGo（WebView2）构建的桌面工具：输入 C 语言结构体定义，递归展开嵌套
字段并导出带格式的 Excel 字段表；另含“表格转 X-Macro”工具。React + TypeScript +
Vite 前端位于仓库根目录，Go 桌面壳与业务后端位于 `src-mygo/`。

> 历史实现：`src-gpui/` 是早期的 Rust + GPUI 版本，功能对齐，作为迁移期对照与
> 回退保留。现行开发以 MyGo 版为准，见
> [MyGo 重构方案](docs/dev/mygo-refactoring-plan.md)。

导出 `TR2_DD_IN` / `TR2_DD_OUT` 字段表时，默认文件名会自动规范为
`TR2_SW_DD_IN.xlsx` / `TR2_SW_DD_OUT.xlsx`；其他导出文件也不再附加中文“字段表”后缀。

## 功能

- 解析 `typedef struct { ... } Name;`、`typedef struct Tag { ... } Name;` 和 `struct Name { ... };`
- 可直接粘贴完整 `.h` 文件，自动忽略 include guard、`#include`、函数声明等非结构体内容
- 扫描头文件中的所有结构体，并允许从下拉框选择任意一个作为导出目标
- 解析整数 `#define` 常量、基础类型 `typedef`、多别名/指针别名及常见编译器属性
- 递归展开按值嵌套的结构体与结构体定长数组
- 识别常用 C 基础类型、定长数组、指针、位字段及字段注释
- 按常见 64 位 C ABI 计算结构体大小和字段 Offset，支持 `#pragma pack`、`packed`、位字段和命名 union
- 根结构体可切换；未指定时默认选择代码中的最后一个结构体
- 导出 `.xlsx`，包含黄色表头、边框、列宽、冻结首行和自动筛选
- 首页字段表预览参考原版的列型和对齐方式，并收紧列宽；较窄窗口可水平滚动
- 所有解析及导出均在本机完成
- 内置“表格转 X-Macro”功能，可读取 `.xlsx/.csv` 或直接粘贴从 Excel/WPS 复制的单元格，
  生成 `X(type, name, offset, size)` 代码
- 标题栏和工具栏显示当前版本号
- 隐藏原生标题栏，页面自绘标题栏（原生拖动）与窗口控制按钮

## 开发运行（MyGo）

需要 Go 1.27+ 与 Bun 1.4.5，Windows 上需要 WebView2 Runtime。在项目根目录运行：

```bash
bun install --frozen-lockfile
bun run dev:mygo
```

`dev:mygo` 会启动 Vite 开发服务器并用 MyGo 打开窗口。修改前端即时热更新；修改
`src-mygo/` 下的 Go 代码后重新运行即可。

## 构建

```bash
# 前端类型检查 + 生产构建
bun run build

# 打包桌面应用（前端嵌入 exe，跳过安装包）
cd src-mygo && go tool mygo build -skip-installer
```

构建产物位于 `src-mygo/dist/windows-<arch>/StructSheet.exe`。生成的 TypeScript
绑定位于 `src/lib/mygo/bindings.ts`（由 `mygo generate` 输出，勿手动编辑）。

## 测试

```bash
# Go 后端（解析、导出、表格转换、日志）
cd src-mygo && go test ./...

# 前端类型检查
bun run check:static

# 前端单元测试
bun run test:unit
```

## 历史 GPUI 版本（对照/回退）

```powershell
cargo run --manifest-path src-gpui/Cargo.toml
cargo build --manifest-path src-gpui/Cargo.toml --release
```

构建结果位于 `src-gpui/target/release/`。Windows ARM64 开发机若缺少 native `ring`
所需的 Clang，可加 `--target x86_64-pc-windows-msvc`。

## 表格转 X-Macro

字段表至少需要 `Parameter Name`（或“字段名”）和 `Type`（或“数据类型”）两列。
`Bytes/Offset` 列支持 `0~3`、`0-3` 或单个偏移值；没有显式 Size 时会按常见 C 类型
推断字节数。

代码名称只需填写一次公共部分。例如输入 `EI_TO_HMGPM`，程序会自动生成
`EI_TO_HMGPM_FIELDS` 宏和 `EI_TO_HMGPM_TYPE` 结构体类型。

也可以在 Excel/WPS 中选中包含表头的单元格区域，复制后直接粘贴到“表格转代码”窗口。

> 说明：MyGo 版当前直接读取 `.xlsx` 与 `.csv`；`.xls/.xlsb/.ods` 会提示先另存为
> `.xlsx`/`.csv`（后续版本再补充纯 Go 读取器）。

## 偏移量说明

当前按 Windows x64 常见 ABI 计算：指针为 8 字节，`long` 为 4 字节；支持自然对齐、
`#pragma pack(push/pop, n)`、`__attribute__((packed))`、连续位字段和命名 union。
复杂编译器专属对齐扩展或无法求值的宏表达式仍可能需要按目标编译器调整。
