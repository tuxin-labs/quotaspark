import { useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

type Phase = "idle" | "checking" | "uptodate" | "available" | "downloading" | "ready" | "error";

/**
 * 设置弹窗里的「检查更新」区块。
 * 依赖 tauri-plugin-updater：端点与公钥在 src-tauri/tauri.conf.json 的 plugins.updater；
 * 发布流程见 README「发布与自动更新」。
 */
export default function UpdaterSection() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [current, setCurrent] = useState("");
  const [message, setMessage] = useState("");
  const [progress, setProgress] = useState<number | null>(null);
  const updateRef = useRef<Update | null>(null);

  useEffect(() => {
    getVersion()
      .then(setCurrent)
      .catch(() => setCurrent("未知"));
  }, []);

  const checkUpdate = async () => {
    setPhase("checking");
    setMessage("");
    try {
      const u = await check();
      if (!u) {
        updateRef.current = null;
        setPhase("uptodate");
        setMessage("已是最新版本");
        return;
      }
      updateRef.current = u;
      setPhase("available");
      setMessage(`发现新版本 ${u.version}${u.body ? `：${u.body}` : ""}`);
    } catch (e) {
      setPhase("error");
      setMessage(
        `检查失败：${e}。开发模式下或「设置 → 更新源」尚未指向有效的 latest.json 时属预期。`,
      );
    }
  };

  const downloadAndInstall = async () => {
    const u = updateRef.current;
    if (!u) return;
    setPhase("downloading");
    setProgress(0);
    let total = 0;
    let received = 0;
    try {
      await u.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            total = event.data.contentLength ?? 0;
            break;
          case "Progress":
            received += event.data.chunkLength;
            if (total > 0) setProgress(Math.min(100, Math.round((received / total) * 100)));
            break;
          case "Finished":
            setProgress(100);
            break;
        }
      });
      setPhase("ready");
      setMessage("更新包已下载并安装完成，重启应用后生效");
    } catch (e) {
      setPhase("error");
      setMessage(`下载失败：${e}`);
    }
  };

  return (
    <div className="updater" data-testid="updater-section">
      <div className="updater-row">
        <span className="updater-ver">当前版本 v{current || "…"}</span>
        <button
          disabled={phase === "checking" || phase === "downloading"}
          data-testid="check-update-btn"
          onClick={checkUpdate}
        >
          {phase === "checking" ? "检查中…" : "检查更新"}
        </button>
      </div>

      {message && (
        <p className={phase === "error" ? "updater-msg err" : "updater-msg"}>{message}</p>
      )}

      {phase === "available" && (
        <button className="primary" data-testid="download-update-btn" onClick={downloadAndInstall}>
          下载并安装
        </button>
      )}

      {phase === "downloading" && progress !== null && (
        <div className="bar">
          <div className="bar-fill" style={{ width: `${progress}%` }} />
        </div>
      )}

      {phase === "ready" && (
        <button className="primary" data-testid="relaunch-btn" onClick={() => relaunch()}>
          重启完成更新
        </button>
      )}
    </div>
  );
}
