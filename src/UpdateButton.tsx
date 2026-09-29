import { useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Phase = "idle" | "checking" | "available" | "downloading" | "ready";

/**
 * 顶栏「检查更新」按钮：状态直接体现在按钮文案上
 * （检查中… → 下载并安装 vX → 下载中 n% → 重启完成更新），详细信息走通知条。
 * 端点与公钥配置在 src-tauri/tauri.conf.json 的 plugins.updater。
 */
export default function UpdateButton({ onNotice }: { onNotice: (msg: string) => void }) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [label, setLabel] = useState("检查更新");
  const updateRef = useRef<Update | null>(null);

  const checkUpdate = async () => {
    setPhase("checking");
    setLabel("检查中…");
    try {
      const u = await check();
      if (!u) {
        reset();
        onNotice("已是最新版本");
        return;
      }
      updateRef.current = u;
      setPhase("available");
      setLabel(`下载并安装 v${u.version}`);
      onNotice(`发现新版本 v${u.version}${u.body ? `：${u.body}` : ""}`);
    } catch (e) {
      reset();
      onNotice(`检查更新失败：${e}（开发模式或更新源未配置时属预期）`);
    }
  };

  const reset = () => {
    setPhase("idle");
    setLabel("检查更新");
  };

  const downloadAndInstall = async () => {
    const u = updateRef.current;
    if (!u) return;
    setPhase("downloading");
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
            if (total > 0) setLabel(`下载中 ${Math.min(100, Math.round((received / total) * 100))}%`);
            break;
          case "Finished":
            setLabel("安装完成");
            break;
        }
      });
      setPhase("ready");
      setLabel("重启完成更新");
      onNotice("更新包已下载并安装完成，点「重启完成更新」生效");
    } catch (e) {
      reset();
      onNotice(`下载失败：${e}`);
    }
  };

  const onClick =
    phase === "ready" ? () => relaunch() : phase === "available" ? downloadAndInstall : checkUpdate;

  return (
    <button
      className={phase === "available" || phase === "ready" ? "primary" : ""}
      disabled={phase === "checking" || phase === "downloading"}
      title="检查并安装新版本"
      data-testid="check-update-btn"
      onClick={onClick}
    >
      {label}
    </button>
  );
}
