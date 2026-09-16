import { invoke, Channel } from "@tauri-apps/api/core";
import type { RepoCmds } from "./repoCmds";

export interface RepoEntry {
  path: string;
  name: string;
  /** 本地自定义显示名（如中文名），空串表示未设置 */
  alias: string;
  added_at: number;
  last_used: number;
  /** 一键运行：测试命令，空串表示未设置（旧配置可能缺省） */
  test_cmd?: string;
  /** 一键运行：构建命令，空串表示未设置 */
  build_cmd?: string;
  /** 一键运行：开发调试命令，空串表示未设置（此时用自动识别结果预填） */
  dev_cmd?: string;
  /** 一键运行：安装依赖命令，空串表示未设置（此时用自动识别结果预填） */
  install_cmd?: string;
  /** 定时自动同步间隔（分钟），0 = 关闭或"每天定时"模式（旧配置可能缺省） */
  auto_sync_mins?: number;
  /** "每天定时"模式的执行时刻（本地时区 "HH:MM"），非空即每天定时模式 */
  daily_time?: string;
  /** 检测到云端更新后是否自动拉取 */
  auto_pull?: boolean;
  /** 上次自动检查时间戳（秒），0 表示从未跑过 */
  last_auto_check?: number;
  /** 上次自动同步结果摘要（"ok:…" / "behind:…" / "skip:…" / "err:…"） */
  last_auto_result?: string;
  /** 用户置顶：排在列表最前（旧配置可能缺省） */
  pinned?: boolean;
}

export interface RepoStatus {
  is_repo: boolean;
  branch: string;
  ahead: number;
  behind: number;
  dirty: boolean;
  has_upstream: boolean;
  remote_url: string;
}

export interface OpResult {
  ok: boolean;
  message: string;
  output: string;
  used_proxy: string | null;
  status: RepoStatus | null;
  need_stash: boolean;
  changes: UpdateChanges | null;
  /** 发现分叉时的结构化信息（检测/拉取/推送三处都可能带） */
  fork?: ForkInfo | null;
  /** 本次变基刚建好的备份分支名：有这个值才给"撤销变基"（后端只在变基动作里返回） */
  backup_branch?: string | null;
}

export interface ProgressEvent {
  stage: string;
  detail: string;
  percent: number | null;
}

export interface UpdateChanges {
  commits: string[];
  diff_stat: string;
}

/** 本地与云端各自都有对方没有的提交时的结构化信息（后端算好，前端不做字符串匹配） */
export interface ForkInfo {
  ahead: number;
  behind: number;
  dirty: boolean;
  branch: string;
  /** 上游分支全名，如 origin/main */
  upstream: string;
  /** 本地未推送的提交（@{u}..HEAD） */
  local_commits: string[];
  /** 云端新增的提交（HEAD..@{u}） */
  remote_commits: string[];
  /** 非空时卡片只说明原因、不给动作（仓库正处于用户自己的变基/合并中途） */
  blocked_reason: string | null;
}

export interface ScanItem {
  path: string;
  status: RepoStatus;
}

export interface CloneOutcome {
  path: string;
  name: string;
  repos: RepoEntry[];
}

export interface AppSettings {
  manual_proxy: string;
  last_good_proxy: string | null;
  theme: string;
  /** 后台自动同步结果是否发系统通知（默认开） */
  notify_auto_sync: boolean;
}

export interface FileChange {
  code: string;
  path: string;
}

export interface CommitInfo {
  hash: string;
  author: string;
  date: string;
  summary: string;
}

export interface CreateOutcome {
  path: string;
  name: string;
  repos: RepoEntry[];
  message: string;
}

export interface AppCredentials {
  git_name: string;
  git_email: string;
  github_token: string;
  gitee_token: string;
  ssh_key_path: string;
}

export interface DirItem {
  name: string;
  is_dir: boolean;
}

export interface LangStat {
  language: string;
  percent: number;
}

export interface DirSize {
  name: string;
  path: string;
  size: number;
  /** 属于白名单内的可清理缓存目录 */
  is_cache: boolean;
}

/** 工作区里的大文件（不含 .git 与缓存目录，那些的体积由 dirs 单独展示） */
export interface BigFile {
  /** 相对仓库根目录的路径，分隔符统一为 / */
  name: string;
  /** 所在文件夹的绝对路径：open_in_folder 只接受目录 */
  dir: string;
  size: number;
}

export interface IgnoreSuggestion {
  /** 顶层目录名 */
  name: string;
  /** 目录里已有被 git 跟踪的文件：写 .gitignore 不会取消跟踪，得靠 git rm -r --cached */
  tracked: boolean;
}

export interface SpaceReport {
  /** 仓库文件夹总占用（字节） */
  total: number;
  dirs: DirSize[];
  /** 最大的若干个工作区文件 */
  largest: BigFile[];
  /** 存在但没被 git 忽略的缓存目录 */
  ignorable: IgnoreSuggestion[];
}

export interface CmdDetect {
  test_cmd: string;
  build_cmd: string;
  dev_cmd: string;
  install_cmd: string;
}

/** 后台定时自动同步的事件（scheduler.rs 推送） */
export interface AutoSyncEvent {
  path: string;
  name: string;
  kind: "pulled" | "behind" | "ok" | "skip" | "error";
  message: string;
  commits: string[];
  diff_stat: string;
  repos: RepoEntry[];
}

export const api = {
  listRepos: () => invoke<RepoEntry[]>("list_repos"),
  addRepo: (path: string) => invoke<RepoEntry[]>("add_repo", { path }),
  removeRepo: (path: string) => invoke<RepoEntry[]>("remove_repo", { path }),
  renameRepo: (path: string, alias: string) => invoke<RepoEntry[]>("rename_repo", { path, alias }),
  listDirItems: (path: string) => invoke<DirItem[]>("list_dir_items", { path }),
  initRepo: (path: string, excludes: string[]) => invoke<RepoEntry>("init_repo", { path, excludes }),
  languages: (path: string) => invoke<LangStat[]>("repo_languages", { path }),
  status: (path: string) => invoke<RepoStatus>("repo_status", { path }),
  checkUpdate: (path: string, onProgress: Channel<ProgressEvent>) =>
    invoke<OpResult>("check_update", { path, onProgress }),
  pull: (path: string, stash: boolean, onProgress: Channel<ProgressEvent>) =>
    invoke<OpResult>("pull_repo", { path, stash, onProgress }),
  push: (path: string) => invoke<OpResult>("push_repo", { path }),
  /** 备份并变基到云端：动手前强制建备份分支，冲突自动中止复原 */
  rebaseFork: (path: string, stash: boolean, onProgress: Channel<ProgressEvent>) =>
    invoke<OpResult>("rebase_fork", { path, stash, onProgress }),
  /** 撤销上次变基：硬回退到备份分支（界面需先二次确认，会丢弃变基后的提交） */
  undoRebase: (path: string) => invoke<OpResult>("undo_rebase", { path }),
  commitPush: (path: string, message: string) => invoke<OpResult>("commit_push", { path, message }),
  listChanges: (path: string) => invoke<FileChange[]>("list_changes", { path }),
  recentCommits: (path: string) => invoke<CommitInfo[]>("recent_commits", { path }),
  clone: (url: string, parentDir: string, onProgress: Channel<ProgressEvent>, netMode = "auto") =>
    invoke<CloneOutcome>("clone_repo", { url, parentDir, onProgress, netMode }),
  createRepo: (name: string, parentDir: string, platform: string, isPrivate: boolean, description: string) =>
    invoke<CreateOutcome>("create_repo", { name, parentDir, platform, private: isPrivate, description }),
  scanAll: () => invoke<ScanItem[]>("scan_all"),
  getSettings: () => invoke<AppSettings>("get_settings"),
  setSettings: (manualProxy: string) => invoke<void>("set_settings", { manualProxy }),
  setTheme: (theme: string) => invoke<void>("set_theme", { theme }),
  /** 系统通知总开关，即时生效 */
  setNotifyAutoSync: (enabled: boolean) =>
    invoke<void>("set_notify_auto_sync", { enabled }),
  getCredentials: () => invoke<AppCredentials>("get_credentials"),
  setCredentials: (c: {
    gitName: string;
    gitEmail: string;
    githubToken: string;
    giteeToken: string;
    sshKeyPath: string;
  }) =>
    invoke<void>("set_credentials", {
      gitName: c.gitName,
      gitEmail: c.gitEmail,
      githubToken: c.githubToken,
      giteeToken: c.giteeToken,
      sshKeyPath: c.sshKeyPath,
    }),
  openInFolder: (path: string) => invoke<void>("open_in_folder", { path }),
  openInTerminal: (path: string) => invoke<void>("open_in_terminal", { path }),
  openReadme: (path: string) => invoke<void>("open_readme", { path }),
  openRepoWebsite: (path: string) => invoke<void>("open_repo_website", { path }),
  cancelBusyOp: () => invoke<void>("cancel_busy_op"),
  /** 保存四条一键运行命令；按名字传参，四个字段互相独立 */
  setRepoCmds: (path: string, cmds: RepoCmds) =>
    invoke<RepoEntry[]>("set_repo_cmds", {
      path,
      testCmd: cmds.test,
      buildCmd: cmds.build,
      devCmd: cmds.dev,
      installCmd: cmds.install,
    }),
  setAutoSync: (path: string, mins: number, autoPull: boolean, dailyTime = "") =>
    invoke<RepoEntry[]>("set_auto_sync", { path, mins, autoPull, dailyTime }),
  pinRepo: (path: string, pinned: boolean) =>
    invoke<RepoEntry[]>("pin_repo", { path, pinned }),
  detectCmds: (path: string) => invoke<CmdDetect>("detect_cmds", { path }),
  runRepoCmd: (path: string, cmd: string, onLine: Channel<string>) =>
    invoke<void>("run_repo_cmd", { path, cmd, onLine }),
  stopRepoCmd: () => invoke<void>("stop_repo_cmd"),
  scanRepoSpace: (path: string) => invoke<SpaceReport>("scan_repo_space", { path }),
  deleteCacheDir: (path: string, name: string) => invoke<number>("delete_cache_dir", { path, name }),
  /** 把缓存目录追加进 .gitignore，返回真正写入的条目（已被忽略的会跳过） */
  addGitignoreEntries: (path: string, entries: string[]) =>
    invoke<string[]>("add_gitignore_entries", { path, entries }),
  probe: (addr: string) => invoke<boolean>("probe_proxy", { addr }),
  autostartIsEnabled: () => invoke<boolean>("plugin:autostart|is_enabled"),
  autostartEnable: () => invoke<void>("plugin:autostart|enable"),
  autostartDisable: () => invoke<void>("plugin:autostart|disable"),
};

/** 从远端 URL 识别托管平台，用于界面展示标签 */
export function remoteHost(url: string): "gitee" | "github" | "other" | null {
  if (!url) return null;
  const u = url.toLowerCase();
  if (u.includes("gitee.com")) return "gitee";
  if (u.includes("github.com")) return "github";
  return "other";
}
