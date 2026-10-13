$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$version = (Get-Content -LiteralPath (Join-Path $repoRoot 'package.json') -Raw | ConvertFrom-Json).version
$packageName = "StructSheet-MyGo-v$version-$(Get-Date -Format yyyyMMdd)-Win10-x64-portable"
$packageDir = Join-Path $repoRoot "release\$packageName"
$zipPath = Join-Path $repoRoot "release\$packageName.zip"

Push-Location $repoRoot
try {
  if (-not (Get-Command go -ErrorAction SilentlyContinue)) { throw '需要 Go 1.27 或更新版本，并将 go 加入 PATH。' }
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw '需要 Rust 工具链。' }
  if (-not (Get-Command npm -ErrorAction SilentlyContinue)) { throw '需要 Node.js 与 npm。' }

  npm run build
  if ($LASTEXITCODE -ne 0) { throw '前端构建失败。' }
  cargo build --manifest-path src-core-cli/Cargo.toml --release --target x86_64-pc-windows-msvc
  if ($LASTEXITCODE -ne 0) { throw 'Rust 处理核心构建失败。' }

  New-Item -ItemType Directory -Path $packageDir -Force | Out-Null
  $previousGoOs = $env:GOOS
  $previousGoArch = $env:GOARCH
  try {
    $env:GOOS = 'windows'
    $env:GOARCH = 'amd64'
    go build -ldflags '-H=windowsgui' -o (Join-Path $packageDir 'StructSheet.exe') .
    if ($LASTEXITCODE -ne 0) { throw 'MyGo 程序构建失败。' }
  } finally {
    $env:GOOS = $previousGoOs
    $env:GOARCH = $previousGoArch
  }

  Copy-Item -LiteralPath 'src-core-cli\target\x86_64-pc-windows-msvc\release\StructSheetCore.exe' -Destination (Join-Path $packageDir 'StructSheetCore.exe') -Force
  @'
StructSheet MyGo 便携版（Windows 10/11 x64）

双击 StructSheet.exe 启动，无需安装，不会弹出终端窗口。请将 StructSheet.exe 与 StructSheetCore.exe 放在同一目录。
Windows 10 需要 WebView2 Runtime；Windows 11 通常已内置。

首页可解析 C 结构体并导出 Excel；表格转 X-Macro 支持文件或粘贴内容；Excel 阅读器可打开 xlsx/xls/xlsb/ods/csv。
界面、文件选择由 MyGo 提供；解析、导出和表格读取沿用项目中的 Rust 处理核心。
所有表格与代码内容都在本机处理。
'@ | Set-Content -LiteralPath (Join-Path $packageDir '使用说明.txt') -Encoding utf8

  Compress-Archive -LiteralPath $packageDir -DestinationPath $zipPath -CompressionLevel Optimal -Force
  Write-Output "便携包：$zipPath"
  Write-Output "SHA256：$((Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash)"
} finally {
  Pop-Location
}
