; ============================================================================
;  青鸟 Git —— Inno Setup 6 安装脚本
; ============================================================================
;
;  ## 怎么编出来的
;
;    1) cargo build --workspace --release      ->  target\release\qingbird-git.exe
;    2) ISCC.exe /DMyAppExe=<那个 exe> /DSetupIcon=<那个 ico> installer.iss
;
;  一条命令版见 `scripts/build_installer.ps1`（它自己去 target 下找产物）。
;
;  ## 为什么是这个工具而不是别的
;
;  查过本机：Inno Setup 6 **已装**（%LOCALAPPDATA%\Programs\Inno Setup 6），
;  NSIS / WiX / cargo-wix / cargo-bundle **一个都没有**。Inno 的好处是
;  单个 exe、**免 UAC 的按用户安装**开箱即用、中文可以自己覆盖文案。
;
;  ## 为什么安装包只有**一个**文件
;
;  实查过：运行期**一个文件都不读**——
;  · `theme.rs:44` 配色用 `include_str!("../theme.json")`（刻意不读磁盘）
;  · `app_assets.rs:29` 图标用 `include_bytes!`
;  · `tray.rs:246` 托盘位图来自 `OUT_DIR`，构建期就编进 exe 了
;  · 全项目 `fs::read` / `File::open` / `current_exe` **零命中**
;
;  用户配置在 `%APPDATA%\com.qingbird.git\config.json`，**在安装目录之外**，
;  所以卸载不会带走它（要带走见下面的 `UninstallDelete` 注释）。
;
;  ## 改安装目录 / 图标 / 版本号
;
;  只需要改 [Setup] 与顶部的 #define 那一段，**不用动 [Files]**——
;  exe 路径是从命令行 `/DMyAppExe=` 传进来的（ISCC 的预处理定义）。
; ============================================================================

#ifndef MyAppExe
  #error 请用 ISCC.exe /DMyAppExe=<release 版 exe 路径> 编译，别直接双击本文件
#endif
#ifndef SetupIcon
  #error 请用 ISCC.exe /DSetupIcon=<app.ico 路径> 编译
#endif

#define MyAppName       "青鸟 Git"
#define MyAppNameEn     "Qingbird Git"
#define MyAppVersion    "0.1.0"
#define MyAppPublisher  "qingbird"
#define MyAppExeName    "qingbird-git.exe"
#define MyAppMutex      "qingbird-git-single-instance"

[Setup]
; 这一个 GUID 决定"是不是同一个应用的升级"。**别随手改**——改了就变成并存的
; 另一份安装记录，升级会变成装出两个。
AppId={{8F1B2A63-4C7E-4D5A-9E31-6B0C2D7A9F14}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
; 按用户安装，装到自己的 AppData，**不弹 UAC**。
; 想改成全机安装（要管理员）：把这两行换成
;   PrivilegesRequired=admin
;   DefaultDirName={autopf}\{#MyAppName}
; 并删掉 PrivilegesRequiredOverridesAllowed 那一行。
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DisableProgramGroupPage=yes
; ⚠️ `OutputDir` 与 `MessagesFile` 都相对 **ISCC 的工作目录**解析，
; **不是相对本文件**。所以这里写的是「相对仓库根」的路径 —— 用 `scripts\build_installer.ps1`
; 跑时它会先 `Set-Location` 到仓库根。直接在别处双击/调用会找不到文件。
OutputDir=dist
OutputBaseFilename=qingbird-git-Setup-{#MyAppVersion}
; 图标直接用 build.rs 编出来的那七档 .ico —— **和 exe 里嵌的是同一张源图**，
; 所以安装向导、桌面、任务栏三处长得一样
SetupIconFile={#SetupIcon}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; 应用还在跑时关掉它再覆盖（走 Windows Restart Manager）
CloseApplications=yes
RestartApplications=no
Uninstallable=yes
ChangesEnvironment=no
ChangesAssociations=no
ShowLanguageDialog=no
VersionInfoVersion={#MyAppVersion}
VersionInfoDescription={#MyAppName} 安装程序

[Languages]
; Inno Setup 6 官方发行里**没有**中文语言包（`Languages\` 下 30 个里没有它，
; 第三方翻译不随安装器分发，见 Default.isl 头上给的 istrans 下载地址）。
;
; 这里用的是社区维护的完整简体翻译：
;   https://github.com/kira-96/Inno-Setup-Chinese-Simplified-Translation
;   （维护者 Zhenghan Yang，针对 Inno 6.5.0+；本机编译器 6.7.3）
; 已 `git clone` 下来存成 `packaging\lang\zhCN.isl`。
;
; ⚠️ **那个仓库的文件没有 UTF-8 BOM**，直接用会让 Inno 按系统 ANSI（本机 GBK）读，
; 每一句中文都变乱码——而 ISCC **照样报 "Successful compile"**。
; `scripts\build_installer.ps1` 每次编之前都会补 BOM，不用手动记。
Name: "chinesesimp"; MessagesFile: "packaging\lang\zhCN.isl"

[CustomMessages]
; ⚠️ `{cm:xxx}` **只查 `[CustomMessages]`，不查语言文件里的 `[Messages]`**——
; 名字看起来在 Default.isl 里（CreateDesktopIcon / AdditionalIcons 确实在），
; 照着抄会得到 "A custom message named ... has not been defined"。
; 所以任务页与 [Run] 的文案走这里，**不要**与内置消息同名，免得撞车。
chinesesimp.DesktopIconTask=创建桌面快捷方式
chinesesimp.AdditionalIconsGroup=附加图标：
chinesesimp.RunNowAfterInstall=现在启动 %1

[Tasks]
Name: "desktopicon"; Description: "{cm:DesktopIconTask}"; GroupDescription: "{cm:AdditionalIconsGroup}"; Flags: checkedonce

[Files]
Source: "{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{group}\卸载 {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:RunNowAfterInstall}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; ⚠️ 删掉"安装目录里除本程序外的其余东西"，让卸载干净。
; **不要**在这里删 `%APPDATA%\com.qingbird.git`——那是用户的配置、令牌与
; 仓库列表，默认保留。真要"连设置一起清干净"应该是应用内的一个动作，
; 不是卸载器顺手替用户做的决定。
Type: filesandordirs; Name: "{app}"

[Code]
{ 单实例：装的时候如果应用正在运行，Inno 的 CloseApplications 会先关它。 }
function InitializeSetup(): Boolean;
begin
  Result := True;
end;
