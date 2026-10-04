//! 扫描结果缓存的**界面侧接线**（阶段 16）。
//!
//! 落库的活儿在 core 的 `scan_cache.rs`；这里只做两件事：
//! **读缓存填进界面状态**、**扫完把状态写回缓存**。
//!
//! ## 为什么要分一个文件
//!
//! 读缓存要碰 `views` / `space` / `profile` 三个状态槽，写缓存要在
//! `refresh_many` / `scan_space` / `scan_profile` 三处各调一次。散着写在
//! 那些函数里，第三条路径（以后加"启动时预热"之类的）就会重写一遍。
//! **同一个"缓存 ↔ 界面状态"的映射只该有一份**。
//!
//! ## 界面上"缓存"与"新鲜数据"的关系
//!
//! 缓存**不是**"这次不算、下次再说"，它是**先上屏、再刷新**：
//! 切仓库 / 启动时先把库里的那一份灌进去（页面立刻稳），再起后台任务
//! 扫真的。扫完写回库。
//!
//! 所以扫描期间 `scanning` 是 `true` 而界面**仍然显示旧数**——
//! `space_body` 与 `panels` 里那些判据都要写成
//! `scanning && 还没有内容` 才出骨架屏，**写成光看 `scanning` 会把
//! 已经上屏的缓存顶掉**，那正好和做缓存的目的一样（见 `space_tab.rs:40`）。

use gpui_kit::{Context, SharedString};

use qingbird_core::scan_cache::{self, OpRecord, RepoCache};
use qingbird_core::store;

use crate::state::{rel_time, Op, ProfileState, Shell, SpaceState};

impl Shell {
    /// 启动时把缓存灌进界面状态。**先全部灌上**，这样侧栏徽标、总览统计
    /// 一上来就是有内容的，而不是一片"未扫描"。
    ///
    /// 「空间」「档案」两个槽只跟随**当前选中**的那个仓库，所以只有它
    /// 那一份会进这两个槽；其余仓库只填 `views`（侧栏徽标 / 总览表格
    /// 读的那一层）。
    pub(crate) fn load_caches(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<String> = self.repos.iter().map(|r| r.path.clone()).collect();
        let sel = self.selected.clone();

        for path in paths {
            let Some(c) = scan_cache::load_cache(&path) else { continue };
            if sel.as_deref() == Some(path.as_str()) {
                self.apply_selected_cache(&c, cx);
            }
            self.apply_cache(&path, &c);
        }
    }

    /// 把一份缓存灌进这个仓库的 `view`。
    pub(crate) fn apply_cache(&mut self, path: &str, c: &RepoCache) {
        let v = self
            .views
            .entry(path.to_string())
            .or_insert_with(crate::state::RepoView::blank);
        v.status = c.status.clone();
        v.changed = c.changed;
        v.last_commit_ts = c.last_commit_ts;
        if c.last_commit_ts > 0 {
            v.detected = c.detected.clone();
        }
        // `changes`（未提交文件名）**不进缓存**：那是"此刻"的状态，
        // 缓存里放着昨天的文件名却配今天的 status，比没有更容易骗人。
        // 切到那个仓库时由 `refresh_one` 补上
    }

    /// 把一份缓存灌进「空间」与「档案」两个状态槽。
    ///
    /// ⚠️ 这两个槽**整份换掉，不做合并**。它们跟随的是**当前选中的**
    /// 仓库，切仓库时若把新数据"并"进旧状态，第一个字段对不上就是
    /// **张冠李戴**——仓库 B 的名字底下摆着仓库 A 的体积，比空着坏得多。
    /// 换句话说：**宁可空，不可错**。
    pub(crate) fn apply_selected_cache(&mut self, c: &RepoCache, cx: &mut Context<Self>) {
        // 「已释放 X」那句得留着：清理跑完会立刻重扫，note 要活到那会儿
        // （与 `ops.rs::scan_space` 里那句保留 note 是同一个理由）
        let note = self.space.note.clone();
        self.space = SpaceState {
            report: c.space.clone(),
            note,
            scanned_at: c.scanned_at,
            ..SpaceState::default()
        };
        // 根层直接吃缓存里的 `dirs`——那一层的递归体积刚算过，
        // 不用再扫一遍。没缓存时 `tree_reset` 会把树清空，顺带借
        // `tree_seq` 自增作废在途的子层扫描
        self.tree_reset(cx);

        let mut p = ProfileState {
            scanned_at: c.scanned_at,
            ..ProfileState::default()
        };
        p.langs = c.langs.clone();
        p.activity = c.activity.clone();
        if let (Some(file), Some(text)) = (c.readme.as_ref(), c.readme_text.as_ref()) {
            p.readme_file = Some(file.clone());
            p.readme_text = Some(SharedString::from(text.clone()));
            p.readme_alt = c.readme_alt.clone();
        }
        self.profile = p;
        // 正文视图也要跟着换：切仓库后用户回到「说明」页签，看到的必须是
        // 这个仓库的文档，不是上一个仓库留在渲染器里的那份
        let body = self
            .profile
            .readme_text
            .as_deref()
            .unwrap_or_default()
            .to_string();
        self.readme_state
            .update(cx, |st, cx| st.set_text(&body, cx));
    }

    /// 状态扫描刚跑完：把这个仓库最新的 git 状态**写回缓存**。
    ///
    /// ⚠️ **只写这一条路径的数据**，不碰 `space` / `profile`——
    /// 那两个状态槽跟随的是**选中的**仓库，而这里可能是后台扫的另一个。
    /// 混着写会把 A 仓库的体积记到 B 仓库头上。
    pub(crate) fn persist_view_cache(&mut self, path: &str) {
        let Some(v) = self.views.get(path) else { return };
        // 读回来再改再写：库里可能还带着这个仓库的体积与 README，
        // 从零构造一份会把它们抹掉
        let mut c = scan_cache::load_cache(path).unwrap_or_default();
        c.status = v.status.clone();
        c.changed = v.changed;
        c.last_commit_ts = v.last_commit_ts;
        if v.last_commit_ts > 0 {
            c.detected = v.detected.clone();
        }
        c.scanned_at = qingbird_core::store::now_secs();
        scan_cache::save_cache(path, &c);
    }

    /// `scan_space` 跑完：**只**把体积写回缓存。
    ///
    /// 与 `persist_profile_cache` 分成两个口是有原因的：两个扫描各起各的
    /// 后台任务，谁先跑完是不定的。合在一起写的话，先跑完的那个会拿
    /// 另一个**还没更新的**槽（可能还是上个仓库的）一起落库。
    pub(crate) fn persist_space_cache(&mut self, path: &str) {
        let Some(rep) = self.space.report.clone() else {
            return;
        };
        let mut c = scan_cache::load_cache(path).unwrap_or_default();
        c.space = Some(rep);
        c.scanned_at = self.space.scanned_at;
        scan_cache::save_cache(path, &c);
    }

    /// `scan_profile` 跑完：语言 / 活跃度 / README 进缓存。
    ///
    /// 这三样都是"这个仓库的档案"，**一起存一起取**才不会半新半旧——
    /// 只存语言不存 README 的话，切回这个仓库时 README 那一段空着，
    /// 而语言那一条是有数的，看着像"这仓库没写说明"。
    pub(crate) fn persist_profile_cache(&mut self, path: &str) {
        let mut c = scan_cache::load_cache(path).unwrap_or_default();
        c.langs = self.profile.langs.clone();
        c.activity = self.profile.activity.clone();
        c.readme = self.profile.readme_file.clone();
        c.readme_text = self
            .profile
            .readme_text
            .as_ref()
            .map(|s| s.to_string());
        c.readme_alt = self.profile.readme_alt.clone();
        c.scanned_at = self.profile.scanned_at;
        scan_cache::save_cache(path, &c);
    }

    // ---- 操作历史（`op_history` 表）----

    /// 记一条操作历史，**顺手把内存里那一份也更新掉**。
    ///
    /// ⚠️ 内存那份不是"顺手"——不更新的话，界面上要等下次切仓库
    /// 才看得到刚跑完的这一次，用户会以为"没记上"。
    /// 而从库里回读一遍是纯浪费：刚写进去的东西我们本来就知道。
    ///
    /// **`key()` 给 `None` 的那一类直接不记**（命令槽位，理由见 `Op::key`），
    /// 连内存那份也不动——那一条记的是"仓库被更新过什么"，
    /// 跑完 `cargo test` 覆盖掉上一次拉取的时间，是把话说错了。
    pub(crate) fn record_op(&mut self, path: &str, op: Op, ok: bool, detail: &str) {
        let Some(key) = op.key() else {
            return;
        };
        // 落库失败（没装加速器 / 库坏了）时**仍然更新内存那份**：
        // 这一次操作在用户眼前真的发生了，不显示等于说它没发生
        let ts = scan_cache::record_op(path, key, ok, detail).unwrap_or_else(store::now_secs);
        self.op_logs.insert(
            path.to_string(),
            OpRecord {
                ts,
                path: path.to_string(),
                op: key.to_string(),
                ok,
                detail: detail.to_string(),
            },
        );
    }

    /// 读某个仓库**最近一条**操作历史。
    ///
    /// 切仓库与启动时各调一次，**不在 render 里调**——那是每帧开一次
    /// SQLite 连接。读不到（没记过 / 库坏了）就清掉内存里那一条，
    /// 别留一份上一轮的旧记录。
    pub(crate) fn load_op_log(&mut self, path: &str) {
        match scan_cache::op_history(path, 1).into_iter().next() {
            Some(r) => {
                self.op_logs.insert(path.to_string(), r);
            }
            None => {
                self.op_logs.remove(path);
            }
        }
    }

    /// 界面上要摆的那句话。**空串 = 没什么可说的**（从没记过操作），
    /// 调用方别把空串画出来。
    ///
    /// 成句长这样：
    /// - 成功：「上次拉取 成功 · 3 分钟前」
    /// - 失败：「上次推送 失败 · 远端领先，请先拉取 · 3 分钟前」
    ///
    /// ⚠️ **`detail` 只在失败时出现**——这是实机验出来的，不是设计时就想到的。
    /// core 那边的成功消息本身就是「拉取成功」「推送成功」「没有待推送的提交」
    /// 这类**复述**，拼上去就成了「上次拉取 成功 · 拉取成功 · 刚刚」
    /// （第一轮抓帧就拍到了这一行）。失败时它才是**唯一**有信息量的部分：
    /// 那正是下次要判断"是网络问题还是仓库脏了"时要看的东西。
    ///
    /// 代价：成功但其实什么也没干的推送（`没有待推送的提交`）只显示"成功"。
    /// **这条代价是有意接受的**——刚跑完时屏幕上就有完整输出与结论，
    /// 几分钟后这一行只需要回答"上次成没成"。见路线表阶段 16 的决策记录。
    ///
    /// ⚠️ 摆的位置是 **20px 高的一行**（`log.rs::log_status_row`，超出部分截断），
    /// 所以失败原因还要 `one_line` 压过——git 的报错常带 stderr 原文
    pub(crate) fn op_log_line(&self, path: &str) -> String {
        let Some(o) = self.op_logs.get(path) else {
            return String::new();
        };
        op_line(o.ok, &o.op, &o.detail, &rel_time(o.ts))
    }

    /// 上一次操作**距今多久**（`"2 分钟前"`），没记过就 `None`。
    ///
    /// 给那些只想报时间、不想连操作名一起说的位置用（第三方那行的
    /// 「上次同步」格子）。**别拿它当判据用**——它不区分成没成，
    /// 要那句话的是 `op_log_line`
    pub(crate) fn op_log_when(&self, path: &str) -> Option<String> {
        self.op_logs.get(path).map(|o| rel_time(o.ts))
    }
}

/// 落盘那条记录要显示成的那句话。**抽成纯函数才单测得掉**——
/// `op_log_line` 要一个 `&Shell`，而 Shell 得有 gpui 上下文才建得起来，
/// 于是这条文案就只能靠抓帧看。抽出来之后，"成功那句不被说两遍"
/// 这条规矩变成一条单测（第一轮实机就是在这上面翻的车）。
fn op_line(ok: bool, key: &str, detail: &str, when: &str) -> String {
    let what = Op::label_of_key(key);
    let verdict = if ok { "成功" } else { "失败" };
    if ok {
        return format!("上次{what} {verdict} · {when}");
    }
    let why = one_line(detail, DETAIL_MAX_CHARS);
    if why.is_empty() {
        format!("上次{what} {verdict} · {when}")
    } else {
        // 原因插在时间**前面**：这一行被截断时从右边切，
        // 切掉的应该是"多久以前"，那是最不值钱的
        format!("上次{what} {verdict} · {why} · {when}")
    }
}

/// 失败原因在状态行里最多占几个字。
///
/// 它跟在一句「上次推送 失败」后面，**必须让"做了什么"先占住位置**——
/// 用户扫这一行找的是"上次成没成"，不是失败原因的全文
const DETAIL_MAX_CHARS: usize = 24;

/// 压成**一行**且不超过 `max` 个字（中文按字符算，不按字节）。
///
/// 只留第一行：git 的报错常是"摘要 + 空行 + stderr 原文"三段，
/// 摘要那句已经是人话了，后面是给终端看的。
/// 超长的从**中间**截断并在后面补 `…`：细节里最要紧的通常在两头
/// （"无法访问 xxx" / "请检查网络"），掐掉尾巴等于掐掉结论。
/// `tail` 用 `saturating_sub`——`max` 小到 1 时截不成两半，
/// 与其让它在 debug 里炸，不如只留前半段。
fn one_line(s: &str, max: usize) -> String {
    let s = s.lines().next().unwrap_or("").trim();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return s.to_string();
    }
    let head = max / 2;
    let tail = max.saturating_sub(head + 1);
    let head: String = chars[..head].iter().collect();
    let tail: String = chars[chars.len() - tail..].iter().collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ⚠️ **成功时那句 detail 一律不出现**——这是实机第一轮抓到的帧：
    /// core 的成功消息就是「拉取成功」，拼上去变成
    /// 「上次拉取 成功 · 拉取成功 · 刚刚」，一句话里把同一件事说了两遍。
    /// 这条判据单测得掉，正是因为句式被抽成了纯函数 `op_line`
    #[test]
    fn a_successful_op_does_not_restate_its_own_detail() {
        assert_eq!(
            op_line(true, "pull", "拉取成功", "3 分钟前"),
            "上次拉取 成功 · 3 分钟前",
            "「拉取成功」已经在前两个字里说过了"
        );
        // 连 core 那句带代理说明的也一样：它是复述，不是新信息
        assert_eq!(
            op_line(true, "push", "推送成功（已自动通过代理完成）", "刚刚"),
            "上次推送 成功 · 刚刚"
        );
        // 推了个寂寞也算成功——如实说"成功"是 core 的判定，
        // 我们不在这儿替它改口径
        assert_eq!(
            op_line(true, "push", "没有待推送的提交", "5 分钟前"),
            "上次推送 成功 · 5 分钟前"
        );
    }

    /// 失败时原因**必须出现**，且排在时间前面。
    ///
    /// 这一行会被截断（20px 高），从右边切；"多久以前"是最不值钱的，
    /// 先保住"上次拉取 失败 · 无法连接远端"
    #[test]
    fn a_failed_op_keeps_its_reason_in_front_of_the_time() {
        assert_eq!(
            op_line(false, "pull", "无法连接远端，且未找到可用代理", "3 分钟前"),
            "上次拉取 失败 · 无法连接远端，且未找到可用代理 · 3 分钟前"
        );
        // 失败但没给原因（理论上不该发生）：别把整行搞出多余的间隔号
        assert_eq!(op_line(false, "push", "", "刚刚"), "上次推送 失败 · 刚刚");
        assert_eq!(op_line(false, "push", "   \n ", "刚刚"), "上次推送 失败 · 刚刚");
    }

    /// 认不出来的键**不许变成空白**。库里存着上个版本写的键时，
    /// 空串会被读成"没跑过操作"——那是假话
    #[test]
    fn an_unknown_key_still_produces_a_readable_line() {
        assert_eq!(op_line(true, "pinned:0", "", "刚刚"), "上次某项操作 成功 · 刚刚");
    }

    /// 详情行必须**先保住「做了什么、成没成」**，剩下的位置才留给细节。
    #[test]
    fn detail_stays_on_one_line() {
        // git 失败时的典型长相：摘要 + 空行 + stderr 原文
        assert_eq!(
            one_line("无法访问远程仓库\n\nfatal: unable to access", 24),
            "无法访问远程仓库",
            "只留第一行——后面那段是给终端看的"
        );
        // 单行且不超长：原样，不该被加省略号
        assert_eq!(one_line("没有待推送的提交", 24), "没有待推送的提交");
        // 空 / 纯空白：调用方拿它判断"这句要不要摆"
        assert_eq!(one_line("", 24), "");
        assert_eq!(one_line("   \n  ", 24), "");
    }

    /// 超长时**掐中间、留两头**。
    ///
    /// 掐尾巴等于掐结论：失败原因几乎总是"后半句"（"…请检查网络"），
    /// 只留开头那半句等于没给原因
    #[test]
    fn overlong_detail_loses_its_middle_not_its_ending() {
        let long = "无法访问 https://github.com/acme/very-long-repo-name.git 请检查网络或代理设置";
        let cut = one_line(long, 24);
        assert_eq!(cut.chars().count(), 24, "截完正好 24 个字（`…` 占一个）");
        assert!(cut.starts_with("无法访问"), "开头留住：{cut}");
        assert!(cut.ends_with("设置"), "结尾留住：{cut}");
        assert!(cut.contains('…'), "中间被掐：{cut}");
    }

    /// `max` 给得很小时**不许炸**。这是私有辅助函数、只有一个调用点，
    /// 但它做的是切片下标运算，边界写错就是 debug 构建里的一次 panic。
    /// 留不下两半时**只留前半段**，不硬凑
    #[test]
    fn a_tiny_max_degrades_instead_of_panicking() {
        assert_eq!(one_line("abcdef", 1), "…");
        assert_eq!(one_line("abcdef", 2), "a…", "两字只够「前 + 省略号」");
        assert_eq!(one_line("abcdef", 3), "a…f", "三个字起就能两头各留一点");
        assert_eq!(one_line("abcdef", 4), "ab…f");
    }
}
