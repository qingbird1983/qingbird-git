import type { RepoEntry } from "../api";

/** 仓库列表统一排序（原地）：置顶优先，其余按最近使用自动靠前 */
export function sortRepos(repos: RepoEntry[]): RepoEntry[] {
  return repos.sort(
    (a, b) => Number(b.pinned ?? false) - Number(a.pinned ?? false) || b.last_used - a.last_used,
  );
}
