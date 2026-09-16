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

export interface SpaceReport {
  /** 仓库文件夹总占用（字节） */
  total: number;
  dirs: DirSize[];
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
