import { invoke, Channel } from "@tauri-apps/api/core";

export interface RepoEntry {
  path: string;
  name: string;
  added_at: number;
  last_used: number;
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

export const api = {
  listRepos: () => invoke<RepoEntry[]>("list_repos"),
  addRepo: (path: string) => invoke<RepoEntry[]>("add_repo", { path }),
  removeRepo: (path: string) => invoke<RepoEntry[]>("remove_repo", { path }),
  status: (path: string) => invoke<RepoStatus>("repo_status", { path }),
  checkUpdate: (path: string) => invoke<OpResult>("check_update", { path }),
  pull: (path: string, stash: boolean, onProgress: Channel<ProgressEvent>) =>
    invoke<OpResult>("pull_repo", { path, stash, onProgress }),
  push: (path: string) => invoke<OpResult>("push_repo", { path }),
  commitPush: (path: string, message: string) => invoke<OpResult>("commit_push", { path, message }),
  listChanges: (path: string) => invoke<FileChange[]>("list_changes", { path }),
  recentCommits: (path: string) => invoke<CommitInfo[]>("recent_commits", { path }),
  clone: (url: string, parentDir: string, onProgress: Channel<ProgressEvent>) =>
    invoke<CloneOutcome>("clone_repo", { url, parentDir, onProgress }),
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
