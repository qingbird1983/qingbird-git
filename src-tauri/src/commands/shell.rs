//! 快捷打开（文件夹 / 终端 / README / 仓库主页）与代理探测。

use crate::proxy;
use std::process::Command;

use super::net::remote_url;
use super::repos::valid_repo;

#[tauri::command]
pub fn open_in_folder(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).is_dir() {
        return Err("文件夹不存在".into());
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

/// 在指定目录打开终端；command 非空时在其中执行它（用于「diff 太长，去终端看完整版」这类出口）。
/// command 由调用方拼好，且必须是自己构造的——见 `history::open_commit_diff_in_terminal`，
/// 那里对 hash 与文件路径都做了校验和转义。
pub(super) fn spawn_terminal(path: &str, command: Option<&str>) -> Result<(), String> {
    if !std::path::Path::new(path).is_dir() {
        return Err("文件夹不存在".into());
    }
    let quoted = path.replace('\'', "''");
    let script = match command {
        Some(c) => format!("Set-Location -LiteralPath '{quoted}'; {c}"),
        None => format!("Set-Location -LiteralPath '{quoted}'"),
    };
    // 优先 Windows Terminal（要执行命令时得显式给出 shell 子命令），失败回退 PowerShell
    let wt = || {
        let mut c = Command::new("wt");
        c.arg("-d").arg(path);
        if command.is_some() {
            c.args(["powershell", "-NoExit", "-Command"]).arg(&script);
        }
        c.spawn().is_ok()
    };
    if wt() {
        return Ok(());
    }
    Command::new("cmd")
        .args(["/C", "start", "powershell", "-NoExit", "-Command"])
        .arg(&script)
        .spawn()
        .map_err(|e| format!("无法打开终端：{e}"))?;
    Ok(())
}

#[tauri::command]
pub fn open_in_terminal(path: String) -> Result<(), String> {
    spawn_terminal(&path, None)
}

/// 打开仓库根目录的 README 说明文档（调用系统默认 Markdown 关联程序）
#[tauri::command]
pub fn open_readme(path: String) -> Result<(), String> {
    let root = std::path::Path::new(&path);
    if !root.is_dir() {
        return Err("仓库文件夹不存在".into());
    }
    // 常见命名都识别：大小写变体、.markdown 与无扩展名
    let candidates = [
        "README.md",
        "Readme.md",
        "readme.md",
        "README.markdown",
        "Readme.markdown",
        "README",
        "Readme",
        "readme",
    ];
    let readme = candidates
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.is_file())
        .ok_or_else(|| "仓库根目录未找到 README.md 说明文档".to_string())?;
    tauri_plugin_opener::open_path(&readme, None::<&str>).map_err(|e| format!("打开 README 失败：{e}"))
}

/// 把 origin 远端地址转成浏览器可访问的仓库主页地址：
/// git@host:path 与 ssh:// 形式转 https，http(s) 形式去掉 .git 后缀；无法识别返回 None
pub fn browse_url_from_remote(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        return None;
    }
    let strip_git = |s: &str| s.strip_suffix(".git").unwrap_or(s).to_string();
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        return Some(format!("https://{host}/{}", strip_git(path)));
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest);
        let (host, path) = rest.split_once('/')?;
        return Some(format!("https://{host}/{}", strip_git(path)));
    }
    if url.starts_with("https://") || url.starts_with("http://") {
        return Some(strip_git(url));
    }
    None
}

/// 用系统默认浏览器打开仓库的云端主页（GitHub / Gitee 等）。
/// 不走内置浏览器：WebView2 内嵌页无法复用 git 的代理探测逻辑，也没有可用的网页翻译能力，
/// 而用户默认浏览器自带翻译与自己的代理扩展，体验更好。
#[tauri::command]
pub fn open_repo_website(path: String) -> Result<(), String> {
    if !valid_repo(&path) {
        return Err("该文件夹不是 git 仓库".into());
    }
    let url = remote_url(&path);
    let web = browse_url_from_remote(&url)
        .ok_or_else(|| "该仓库没有可识别的远端地址，无法打开云端主页".to_string())?;
    tauri_plugin_opener::open_url(&web, None::<&str>).map_err(|e| format!("打开浏览器失败：{e}"))
}

#[tauri::command]
pub fn probe_proxy(addr: String) -> Result<bool, String> {
    let url = proxy::to_git_proxy_url(addr.trim());
    Ok(proxy::probe(&url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_url_from_remote_works() {
        assert_eq!(
            browse_url_from_remote("git@github.com:user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
        assert_eq!(
            browse_url_from_remote("ssh://git@gitee.com/user/repo.git"),
            Some("https://gitee.com/user/repo".into())
        );
        assert_eq!(
            browse_url_from_remote("https://github.com/user/repo.git"),
            Some("https://github.com/user/repo".into())
        );
        assert_eq!(
            browse_url_from_remote("https://git.example.com/team/proj/"),
            Some("https://git.example.com/team/proj".into())
        );
        // 无远端 / 非 http 形式（如本地路径）不可浏览
        assert_eq!(browse_url_from_remote(""), None);
        assert_eq!(browse_url_from_remote("C:\\code\\repo"), None);
    }
}
