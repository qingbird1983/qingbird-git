//! 克隆远端仓库到本地。
//!
//! **顺序不能反（D5）：先有 core 才有界面。** 克隆是网络型 git 操作，
//! 必须走 `net::run_net_op`——直连失败自动探测代理回退、凭证注入、
//! 传输停滞保护都在那条链路里。GUI 侧自己拼 `Command::new("git")` 会
//! 把这三样全绕过去。
//!
//! 平移自 `src-tauri/src/commands/clone.rs`，两处与旧版不同：
//! ① Tauri 的 `AppHandle` 换成 core 自己的 `store`；
//! ② 成功入列表走 `repos::add_repo` + `repos::touch_repo`，
//!    不再在这里手搓一条 `RepoEntry`——"加入仓库列表"只有那一个出处。

use std::time::Duration;

use crate::git;
use crate::net::{self, NetMode};
use crate::repos;
use crate::store::RepoEntry;
use crate::sync;
use crate::types::ProgressEvent;

/// 克隆超时。**比拉取/推送宽得多**：大仓库冷克隆十几分钟是常态，
/// 用 90 秒的 `NET_TIMEOUT` 去卡它会把大半正常克隆判成超时
pub const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// 克隆结果
#[derive(Debug, Clone)]
pub struct CloneOutcome {
    /// 克隆出来的仓库绝对路径
    pub path: String,
    /// 仓库名（同时是目标目录名）
    pub name: String,
    /// 写入后的完整仓库列表，供界面直接换掉内存里那份
    pub repos: Vec<RepoEntry>,
}

/// 克隆一个远端仓库到 `parent_dir` 下。
///
/// `on_progress` 传 `None` 时进度静默丢弃。**这个函数是阻塞的**，
/// 调用方必须放后台线程。
///
/// 失败时返回**一句中文结论 + git 原始 stderr**：结论给"该改什么"，
/// stderr 给"到底发生了什么"，两者都要——只给结论用户没法自查，
/// 只给 stderr 用户看不懂。
pub fn clone_repo(
    url: &str,
    parent_dir: &str,
    mode: NetMode,
    on_progress: Option<&dyn Fn(&ProgressEvent)>,
) -> Result<CloneOutcome, String> {
    let Some(_guard) = sync::begin_user_op() else {
        return Err("后台自动同步正在进行，已请求中断，请稍后重试".into());
    };
    let url = url.trim();
    // 只有一个词、既没有冒号也没有斜杠：多半是用户只打了域名
    if !url.contains(':') && !url.contains('/') {
        return Err("仓库地址看起来不完整".into());
    }
    let name = repos::repo_name_from_url(url).ok_or("无法从地址中解析出仓库名（地址末尾应是一个目录名）")?;
    let parent = std::path::Path::new(parent_dir.trim());
    if !parent.is_dir() {
        return Err("目标文件夹不存在".into());
    }
    let dest = parent.join(&name);
    if dest.exists() {
        return Err(format!("目标文件夹已存在：{}", dest.display()));
    }

    let (_tag, forward, stage) = net::progress_hooks(on_progress);
    let (out, used_proxy) = net::run_net_op(
        parent_dir.trim(),
        &["clone", "--progress", url, &name],
        Some(&forward),
        Some(url),
        CLONE_TIMEOUT,
        // **必须 false**：并行的两条路会同时写同一个目标目录，
        // 那是 git 明确不支持的（`allow_race` 的注释就写着这条）
        false,
        Some(&stage),
        mode,
    );
    if !out.success {
        // 进入前已确认目标目录不存在，所以这次的残留一定是本次尝试产生的，
        // 清掉才能让用户改完地址直接重试
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!("{}\n{}", classify(&out, used_proxy.as_deref()), out.stderr.trim()));
    }

    let path = dest.to_string_lossy().to_string();
    // 复用「添加仓库」那一条路径：校验 .git 存在、判重、落盘都在里面，
    // 这里不重复实现一遍
    repos::add_repo(&path)?;
    // 刚克隆完的仓库就是"刚用过"，不置的话它会按 added_at 排到列表末尾，
    // 而它明明是用户此刻最想看的那一个
    repos::touch_repo(&path);

    Ok(CloneOutcome {
        path,
        name,
        repos: repos::list_repos(),
    })
}

/// 把 git 的失败输出翻成一句用户看得懂的中文。
///
/// 顺序有讲究：先认"被取消"，再认代理问题，再认认证，最后才是仓库不存在——
/// 因为同一个远端会同时报好几行（`fatal: could not read Username` 后面
/// 常常还跟着一句 `remote: Repository not found`），**先匹配到的先算**。
/// 取消尤其要放最前：取消时 stderr 里通常没有任何关于仓库的线索，
/// 混进后面的分支就会被报成"仓库不存在"，用户刚点完取消就看到这个。
fn classify(out: &git::GitOutput, used_proxy: Option<&str>) -> String {
    let err = out.stderr.as_str();
    if err.contains(git::CANCELLED_MSG) {
        return "克隆已取消".into();
    }
    if err.contains("未找到可用代理") {
        return "克隆失败：代理模式未找到可用代理，请确认代理软件已运行，或在设置中手动填写代理地址".into();
    }
    // 直连没找到可用代理兜底：这一条要放在认证之前——
    // 连不上的时候 git 有时也会顺带问一句用户名
    if used_proxy.is_none() && git::is_network_error(out) {
        return "克隆失败：无法连接远端，且未找到可用代理".into();
    }
    if err.contains("could not read Username") || err.contains("Authentication failed") {
        return "克隆失败：这是私有仓库，请确认已在本机 Git 凭证管理器中登录".into();
    }
    if err.contains("Repository not found") || err.contains("not found") {
        return "克隆失败：仓库不存在或无权访问".into();
    }
    "克隆失败".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(stderr: &str) -> git::GitOutput {
        git::GitOutput {
            success: false,
            stdout: String::new(),
            stderr: stderr.into(),
            code: 128,
            timed_out: false,
        }
    }

    /// 取消必须排在第一位：取消时 stderr 里通常没有任何仓库线索，
    /// 混进后面的分支就会被报成"仓库不存在"
    #[test]
    fn classify_prefers_cancel_over_everything() {
        let o = out(&format!(
            "{} \nfatal: could not read Username for 'https://h'\nRepository not found",
            git::CANCELLED_MSG
        ));
        assert_eq!(classify(&o, None), "克隆已取消");
    }

    #[test]
    fn classify_maps_the_four_common_failures() {
        assert!(classify(&out("未找到可用代理：扫了 7897/7890/10808"), None).contains("未找到可用代理"));
        assert!(classify(
            &out("fatal: could not read Username for 'https://github.com'"),
            None
        )
        .contains("私有仓库"));
        assert!(classify(&out("remote: Repository not found."), Some("http://127.0.0.1:7897"))
            .contains("仓库不存在"));
    }

    /// 直连打不开远端且没有代理兜底 → 说清"连不上"，
    /// 而不是把它报成认证问题（用户会去检查账号密码，而问题在网络）
    #[test]
    fn classify_reports_no_route_before_auth() {
        let o = out("fatal: unable to access 'https://h/': Failed to connect to h port 443");
        let msg = classify(&o, None);
        assert!(msg.contains("无法连接远端"), "got: {msg}");
        // 已经成功用过代理时不该再说"没有可用代理"——那会误导
        let o2 = out("fatal: unable to access 'https://h/': Failed to connect to h port 443");
        let msg2 = classify(&o2, Some("http://127.0.0.1:7897"));
        assert!(!msg2.contains("未找到可用代理"), "got: {msg2}");
    }

    #[test]
    fn classify_falls_back_to_a_neutral_line() {
        assert_eq!(classify(&out("something else entirely"), None), "克隆失败");
    }
}
