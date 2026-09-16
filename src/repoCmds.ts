/**
 * 一键运行命令（测试 / 构建 / 开发 / 安装依赖）的类型与纯逻辑。
 *
 * 为什么单独抽出来：四个字段的值都是 string，历史上用位置参数传给后端时写反过
 * （保存「测试」把值写进了「构建」，反之亦然），而 TS 编译器对
 * `f(path, cmd, buildCmd, testCmd)` 这种全 string 的位置传参完全无感。
 * 这里改成按名字取值的对象载荷（Record<CmdKind, string>），从结构上杜绝串位。
 */

/** 一键运行命令的种类，与后端 RepoEntry 的 *_cmd 字段一一对应 */
export type CmdKind = "test" | "build" | "dev" | "install";

/** 面板里的展示顺序 */
export const CMD_KINDS: readonly CmdKind[] = ["test", "build", "dev", "install"];

/** 每行命令的界面文案 */
export const CMD_META: Record<CmdKind, { label: string; placeholder: string }> = {
  test: { label: "测试", placeholder: "如：cargo test / pnpm test / pytest" },
  build: { label: "构建", placeholder: "如：cargo build --release / pnpm run build" },
  dev: { label: "开发", placeholder: "如：pnpm run dev / cargo run" },
  install: { label: "安装依赖", placeholder: "如：pnpm install / pip install -r requirements.txt" },
};

/** 四行命令的值，键即字段名 */
export type RepoCmds = Record<CmdKind, string>;

/** 后端返回的已保存命令（旧配置可能整段缺省，缺省当空串） */
export interface StoredCmds {
  test_cmd?: string;
  build_cmd?: string;
  dev_cmd?: string;
  install_cmd?: string;
}

/** 空值：四个字段都是空串，表示全部未设置 */
export function emptyCmds(): RepoCmds {
  return { test: "", build: "", dev: "", install: "" };
}

/**
 * 读取已保存的命令。test_cmd / build_cmd / dev_cmd / install_cmd 是四个独立字段，
 * 缺省与空串同等对待（旧配置文件里没有 dev/install）。
 */
export function cmdsFromEntry(entry: StoredCmds | undefined): RepoCmds {
  return {
    test: entry?.test_cmd ?? "",
    build: entry?.build_cmd ?? "",
    dev: entry?.dev_cmd ?? "",
    install: entry?.install_cmd ?? "",
  };
}

/** 把后端自动识别的结果转成同一形状（字段名与存储字段相同） */
export function cmdsFromDetect(detected: StoredCmds | undefined): RepoCmds {
  return cmdsFromEntry(detected);
}

/** 只补空：已保存的值优先，没保存过的才用自动识别结果预填，用户保存过的永远不被识别结果顶掉 */
export function prefillCmds(saved: RepoCmds, detected: RepoCmds): RepoCmds {
  const pick = (k: CmdKind) => saved[k] || detected[k];
  return { test: pick("test"), build: pick("build"), dev: pick("dev"), install: pick("install") };
}

/** 改其中一行，其余三行原样保留；按名字定位，不存在把 A 写进 B 的可能 */
export function withCmd(cmds: RepoCmds, kind: CmdKind, cmd: string): RepoCmds {
  return { ...cmds, [kind]: cmd };
}

/** 落盘前统一去首尾空白：避免手滑多打的空格被存成命令的一部分 */
export function trimCmds(cmds: RepoCmds): RepoCmds {
  const pick = (k: CmdKind) => cmds[k].trim();
  return { test: pick("test"), build: pick("build"), dev: pick("dev"), install: pick("install") };
}

/**
 * 点某一行「启动」时真正落盘的四条命令：该行用输入框里的最新值，
 * 其余三行保持各自输入框的值（空串=未设置，下次打开用自动识别结果预填）。
 * 面板保存统一走这里，只有这一个入口，改一条不可能连带改掉别的。
 */
export function cmdsToSave(kind: CmdKind, cmd: string, boxes: RepoCmds): RepoCmds {
  return trimCmds(withCmd(boxes, kind, cmd));
}
