<#
.SYNOPSIS
    一条命令出安装包：`qingbird-git-Setup-<版本>.exe`（版本号单源见 Cargo.toml）

.DESCRIPTION
    两步：
      1) cargo build --workspace --release   ->  target\release\qingbird-git.exe
      2) ISCC.exe installer.iss             ->  dist\qingbird-git-Setup-<ver>.exe

    为什么单独一个脚本而不是记在 AGENTS.md 里的一句话：
    **第 2 步的两个路径都是"找出来的"**，不是写死的——
      · exe 在 target\release\ 下（release 与 debug 是两条产物路径，混过一次）
      · .ico 在 target\release\build\qingbird-gui-*/out\ 下，而那个目录名
        **带一串 cargo 算出来的哈希**，改一次依赖就变。写死路径等于埋一颗
        「今天能编、明天编不过」的雷。
    所以这里去现场找，找不到就报错说清楚缺什么，而不是编出一个没图标的包。

.PARAMETER SkipBuild
    跳过 release 编译（已经编过、只想重打安装包时用）

.EXAMPLE
    pwsh scripts/build_installer.ps1
    pwsh scripts/build_installer.ps1 -SkipBuild
#>
[CmdletBinding()]
param([switch]$SkipBuild)

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

# ──  版本号：单源是 Cargo.toml 的 [workspace.package].version ────────────
# 32-13。以前这个数手写在三个地方（gui/Cargo.toml、crates/core/Cargo.toml、
# installer.iss 的 #define MyAppVersion），而**只改一两处不会有任何报错**。
# 现在两个包都 `version.workspace = true` 继承根表，这里读同一处传给 ISCC；
# installer.iss 那边改成了「没传就 #error」，所以绕开本脚本直接跑 ISCC 也不行。
$rootToml = Get-Content 'Cargo.toml' -Raw
if ($rootToml -notmatch '(?s)\[workspace\.package\].*?version\s*=\s*"([^"]+)"') {
    throw "Cargo.toml 里找不到 [workspace.package] 的 version——版本号单源就是那一行"
}
$ver = $Matches[1]
Write-Host "版本: $ver（单源：Cargo.toml [workspace.package].version）"

# ── ISCC 在哪 ────────────────────────────────────────────────────────────
# 查过本机：Inno Setup 6 已装；NSIS / WiX / cargo-wix / cargo-bundle 一个都没有。
# 路径**不写死**：Inno 既能装到 Program Files，也能装到用户的 LocalAppData
# （本机就是后者），两处都试。
$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
    throw "找不到 ISCC.exe（Inno Setup 6）。装一个：https://jrsoftware.org/isdl.php"
}
Write-Host "ISCC: $iscc"

# ── ① release 版 exe ─────────────────────────────────────────────────────
$exe = Join-Path $PWD 'target\release\qingbird-git.exe'
if (-not $SkipBuild) {
    Write-Host "`n==> cargo build --workspace --release（gpui 那一大棵树，约 5 分钟）"
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) { throw "release 编译失败" }
}
if (-not (Test-Path $exe)) {
    throw "没有 $exe`n先跑 `cargo build --workspace --release`（或去掉 -SkipBuild）"
}
Write-Host "exe: $exe ($((Get-Item $exe).Length) 字节)"

# ── ② 那七档 app.ico ─────────────────────────────────────────────────────
# build.rs 的产物。目录名带 cargo 哈希，所以现场找。
$ico = Get-ChildItem 'target\release\build' -Filter 'app.ico' -Recurse -File -ErrorAction SilentlyContinue |
       Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $ico) {
    throw "找不到 release 的 app.ico（target\release\build\qingbird-gui-*\out\）。`n" +
          "多半是 build.rs 没跑成——先确认 gui\Cargo.toml 里有 `build = ""build.rs""`。"
}
Write-Host "ico: $($ico.FullName) ($($ico.Length) 字节)"

# ── ③ 编安装包 ───────────────────────────────────────────────────────────
# .iss 与 .isl 都必须带 UTF-8 BOM，否则 Inno 按系统 ANSI 读、中文全变乱码。
python scripts\add_bom.py installer.iss
python scripts/add_bom.py 'packaging\lang\zhCN.isl'

Write-Host "`n==> ISCC"
& $iscc "/DMyAppVersion=$ver" "/DMyAppExe=$exe" "/DSetupIcon=$($ico.FullName)" installer.iss
if ($LASTEXITCODE -ne 0) { throw "ISCC 失败" }

$setup = Get-ChildItem 'dist' -Filter 'qingbird-git-Setup-*.exe' |
         Sort-Object LastWriteTime -Descending | Select-Object -First 1
Write-Host "`n✅ $($setup.FullName)  $([math]::Round($setup.Length / 1MB, 2)) MB"
