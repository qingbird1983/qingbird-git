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

use qingbird_core::scan_cache::{self, RepoCache};

use crate::state::{ProfileState, Shell, SpaceState};

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
}
