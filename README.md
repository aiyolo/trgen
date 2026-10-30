# StructSheet

一个使用 Rust + GPUI 构建的桌面工具：输入 C 语言结构体定义，递归展开嵌套字段并导出带格式的 Excel 字段表。程序位于 `src-gpui/`，解析、导出和表格读取代码均在其中。

导出 `TR2_DD_IN` / `TR2_DD_OUT` 字段表时，默认文件名会自动规范为 `TR2_SW_DD_IN.xlsx` / `TR2_SW_DD_OUT.xlsx`；其他导出文件也不再附加中文“字段表”后缀。

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
- 首页字段表预览参考原版的列型和对齐方式，并收紧列宽；默认窗口可同时看到全部 8 列，较窄窗口可水平滚动
- 所有解析及导出均在本机完成
- 内置“表格转 X-Macro”功能，可读取 `.xlsx/.xls/.xlsb/.ods/.csv`，也可直接粘贴从 Excel/WPS 复制的单元格，生成 `X(type, name, offset, size)` 代码
- 窗口标题栏和工具栏显示当前版本号

## 开发运行

GPUI 版本需要 Rust 和 Windows C++ 构建工具。在项目根目录运行：

```powershell
cargo run --manifest-path src-gpui/Cargo.toml
```

Windows ARM64 开发机若缺少 native `ring` 所需的 Clang，可在 Visual Studio 开发者终端构建 x64 目标：

```powershell
cargo run --manifest-path src-gpui/Cargo.toml --target x86_64-pc-windows-msvc
```

## 构建

```powershell
cargo build --manifest-path src-gpui/Cargo.toml --release
```

构建结果位于 `src-gpui/target/release/`。

## 表格转 X-Macro

该功能已直接集成在主程序中，无需额外 DLL 或插件目录。

字段表至少需要 `Parameter Name`（或“字段名”）和 `Type`（或“数据类型”）两列。`Bytes/Offset` 列支持 `0~3`、`0-3` 或单个偏移值；没有显式 Size 时会按常见 C 类型推断字节数。

代码名称只需填写一次公共部分。例如输入 `EI_TO_HMGPM`，程序会自动生成 `EI_TO_HMGPM_FIELDS` 宏和 `EI_TO_HMGPM_TYPE` 结构体类型。

也可以在 Excel/WPS 中选中包含表头的单元格区域，复制后直接粘贴到“表格转代码”窗口。剪贴板中的制表符和换行会按行列解析，不需要先保存文件。
粘贴区和生成代码区都支持水平滚动，较宽的表头及代码行无需放大窗口即可查看。

## 偏移量说明

当前按 Windows x64 常见 ABI 计算：指针为 8 字节，`long` 为 4 字节；支持自然对齐、`#pragma pack(push/pop, n)`、`__attribute__((packed))`、连续位字段和命名 union。复杂编译器专属对齐扩展或无法求值的宏表达式仍可能需要按目标编译器调整。
