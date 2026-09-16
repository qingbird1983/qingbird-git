import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { useApp } from "../store";
import type { ThemeMode } from "../lib/theme";
import { IconMoon, IconSun, IconMonitor } from "./icons";

interface Props {
  onClose: () => void;
}

export default function SettingsModal({ onClose }: Props) {
  const { theme, setTheme } = useApp();
  const [addr, setAddr] = useState("");
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [autostart, setAutostart] = useState(false);

  const [gitName, setGitName] = useState("");
  const [gitEmail, setGitEmail] = useState("");
  const [githubToken, setGithubToken] = useState("");
  const [giteeToken, setGiteeToken] = useState("");
  const [sshKeyPath, setSshKeyPath] = useState("");
  const [credSaved, setCredSaved] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        setAddr((await api.getSettings()).manual_proxy);
        setAutostart(await api.autostartIsEnabled());
        const c = await api.getCredentials();
        setGitName(c.git_name);
        setGitEmail(c.git_email);
        setGithubToken(c.github_token);
        setGiteeToken(c.gitee_token);
        setSshKeyPath(c.ssh_key_path);
      } catch {
        /* 忽略 */
      }
    })();
  }, []);

  const handleSaveCreds = async () => {
    await api.setCredentials({
      gitName,
      gitEmail,
      githubToken,
      giteeToken,
      sshKeyPath,
    });
    setCredSaved(true);
    setTimeout(() => setCredSaved(false), 2000);
  };

  const pickKey = async () => {
    const picked = await open({
      multiple: false,
      title: "选择 SSH 私钥文件（如 id_ed25519 / id_rsa）",
    });
    if (typeof picked === "string") setSshKeyPath(picked);
  };

  const handleSave = async () => {
    await api.setSettings(addr.trim());
    onClose();
  };

  const toggleAutostart = async () => {
    try {
      if (autostart) {
        await api.autostartDisable();
        setAutostart(false);
      } else {
        await api.autostartEnable();
        setAutostart(true);
      }
    } catch (e) {
      setResult(`❌ 开机自启设置失败：${e}`);
    }
  };

  const handleTest = async () => {
    if (!addr.trim()) return;
    setTesting(true);
    setResult(null);
    try {
      const ok = await api.probe(addr.trim());
      setResult(ok ? "✅ 该代理可用，可以通过它访问 github.com" : "❌ 该代理不可用（连不上或无法访问 github.com）");
    } catch (e) {
      setResult(`❌ 测试失败：${e}`);
    } finally {
      setTesting(false);
    }
  };

  const themeOptions: { value: ThemeMode; label: string; icon: typeof IconMoon }[] = [
    { value: "dark", label: "暗色", icon: IconMoon },
    { value: "light", label: "亮色", icon: IconSun },
    { value: "system", label: "跟随系统", icon: IconMonitor },
  ];

  return (
    <div className="modal-mask" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>设置</h3>

        <p className="modal-subtitle">外观</p>
        <div className="segmented">
          {themeOptions.map((o) => (
            <button
              key={o.value}
              className={`segment ${theme === o.value ? "segment-active" : ""}`}
              onClick={() => void setTheme(o.value)}
            >
              <o.icon size={13} />
              {o.label}
            </button>
          ))}
        </div>

        <p className="modal-subtitle">系统</p>
        <label className="switch-row">
          <span>开机自动启动</span>
          <button
            className={`switch ${autostart ? "on" : ""}`}
            role="switch"
            aria-checked={autostart}
            onClick={() => void toggleAutostart()}
          >
            <span className="switch-knob" />
          </button>
        </label>

        <p className="modal-subtitle">Git 身份（提交时使用）</p>
        <div className="modal-row">
          <input
            className="modal-input"
            placeholder="姓名，如 Zhang San"
            value={gitName}
            onChange={(e) => setGitName(e.target.value)}
          />
          <input
            className="modal-input"
            placeholder="邮箱，如 me@example.com"
            value={gitEmail}
            onChange={(e) => setGitEmail(e.target.value)}
          />
        </div>

        <p className="modal-subtitle">令牌认证（仅保存在本机配置文件）</p>
        <p className="modal-desc">
          推送私有仓库或云端建仓需要令牌。GitHub 填
          <a
            className="link"
            href="https://github.com/settings/tokens"
            onClick={(e) => {
              e.preventDefault();
              void openUrl("https://github.com/settings/tokens");
            }}
          >
            Personal Access Token
          </a>
          （GitHub → Settings → Developer settings），Gitee 填
          <a
            className="link"
            href="https://gitee.com/profile/personal_access_tokens"
            onClick={(e) => {
              e.preventDefault();
              void openUrl("https://gitee.com/profile/personal_access_tokens");
            }}
          >
            私人令牌
          </a>
          （Gitee → 设置 → 安全设置）。留空则使用本机 Git 凭证管理器。
        </p>
        <div className="modal-col">
          <input
            className="modal-input"
            type="password"
            placeholder="GitHub 访问令牌（PAT）"
            value={githubToken}
            onChange={(e) => setGithubToken(e.target.value)}
          />
          <input
            className="modal-input"
            type="password"
            placeholder="Gitee 私人令牌"
            value={giteeToken}
            onChange={(e) => setGiteeToken(e.target.value)}
          />
          <div className="modal-row">
            <input
              className="modal-input"
              placeholder="SSH 私钥路径（远端为 git@ 地址时使用，可留空）"
              value={sshKeyPath}
              readOnly
              onClick={() => void pickKey()}
            />
            <button className="btn-sm btn-ghost" onClick={() => void pickKey()}>
              选择…
            </button>
          </div>
        </div>

        <p className="modal-subtitle">代理</p>
        <p className="modal-desc">
          通常无需设置：检测到网络不通时，软件会自动探测本机运行的 clash / v2rayN 等代理
          （系统代理、7897 / 7890 / 10808 等常见端口），并仅对当次 git 操作使用。
          <br />
          如果自动探测不理想，可以在这里手动指定，例如 <code>http://127.0.0.1:7890</code> 或{" "}
          <code>socks5://127.0.0.1:1080</code>，留空则恢复全自动。
        </p>
        <div className="modal-row">
          <input
            className="modal-input"
            placeholder="http://127.0.0.1:7890（留空为自动探测）"
            value={addr}
            onChange={(e) => setAddr(e.target.value)}
          />
          <button className="btn-sm" disabled={testing || !addr.trim()} onClick={() => void handleTest()}>
            {testing ? "测试中…" : "测试"}
          </button>
        </div>
        {result && <div className="modal-result">{result}</div>}
        <div className="modal-footer">
          <button className="btn-sm btn-ghost" onClick={onClose}>
            取消
          </button>
          <button className="btn-sm" onClick={() => void handleSaveCreds()}>
            {credSaved ? "✓ 已保存" : "保存身份与令牌"}
          </button>
          <button className="btn-sm btn-ghost" onClick={() => void handleSave()}>
            保存代理并关闭
          </button>
        </div>
      </div>
    </div>
  );
}
