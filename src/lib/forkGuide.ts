import type { ForkInfo } from "../api";

/**
 * 分叉卡片的按钮。`danger` 为真表示会丢弃已经存在的东西（界面需要二次确认）；
 * 「备份并变基」不算危险动作：本地提交只是被重放，内容不丢，备份分支还能一键退回。
 */
export interface ForkAction {
  id: "rebase" | "rebase-stash" | "undo";
  label: string;
  danger: boolean;
  hint: string;
}

export interface ForkView {
  title: string;
  /** 一行事实：两边各有几个提交 */
  summary: string;
  /** 有未提交变更：变基前要先暂存 */
  needsStash: boolean;
  actions: ForkAction[];
  /** 非空时按钮全部收起，只说明原因 */
  blocked: string | null;
}

const undoAction = (branch: string): ForkAction => ({
  id: "undo",
  label: "撤销变基",
  danger: true,
  hint: `回到变基前的状态（备份分支 ${branch}），会丢弃变基之后新产生的提交`,
});

/**
 * 由后端的 fork / 备份分支信息推导卡片该长什么样。
 * 返回 null 表示当前没有需要用户决定的事（卡片不出现）。
 *
 * 两种会出卡片的情况：
 * 1. 分叉了 —— 给合流的出口；
 * 2. 刚做过变基、备份还在 —— 此时已经不是分叉了，但用户最可能在这时后悔。
 */
export function forkView(
  fork: ForkInfo | null | undefined,
  backupBranch: string | null | undefined,
): ForkView | null {
  if (!fork) {
    if (!backupBranch) return null;
    return {
      title: "上次变基的备份还在",
      summary: `本地提交已经重放到云端提交之上。觉得不对可以回到变基前（备份分支 ${backupBranch}）。`,
      needsStash: false,
      actions: [undoAction(backupBranch)],
      blocked: null,
    };
  }

  const parts = [`本地有 ${fork.ahead} 个提交未推送`, `云端有 ${fork.behind} 个新提交`];
  if (fork.dirty) parts.push("工作区还有未提交变更");
  const view: ForkView = {
    title: "本地与云端已分叉",
    summary: `${parts.join("，")}。`,
    needsStash: fork.dirty,
    actions: [],
    blocked: fork.blocked_reason,
  };
  // 仓库卡在用户自己的变基/合并里：只解释原因，一个动作都不给
  if (fork.blocked_reason) return view;

  view.actions = [
    fork.dirty
      ? {
          id: "rebase-stash",
          label: "暂存并变基到云端",
          danger: false,
          hint: "先把改动暂存起来再变基，完成后自动恢复；本地提交会被重放到云端提交之上，动手前会建好备份分支",
        }
      : {
          id: "rebase",
          label: "备份并变基到云端",
          danger: false,
          hint: "把本地提交重放到云端提交之上；动手前会建好备份分支，冲突会自动中止并复原",
        },
  ];
  if (backupBranch) view.actions.push(undoAction(backupBranch));
  return view;
}

/** 提交列表的展示标题，空列表要点明"没有"而不是留白 */
export function commitSectionTitle(kind: "local" | "remote", count: number): string {
  if (kind === "local") return count > 0 ? `本地未推送的提交（${count}）` : "本地没有未推送的提交";
  return count > 0 ? `云端新增的提交（${count}）` : "云端没有新提交";
}
