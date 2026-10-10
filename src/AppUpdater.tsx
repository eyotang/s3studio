import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ArrowDownToLine, RefreshCw, Sparkles, X } from "lucide-react";
import { api, native } from "./api";
import { formatBytes } from "./utils";
import { version } from "../package.json";

type Status = {
  revision: number;
  mode: "disabled" | "inApp" | "manual";
  phase:
    | "disabled"
    | "idle"
    | "checking"
    | "available"
    | "downloading"
    | "installing";
  currentVersion: string;
  version: string | null;
  notes: string | null;
  downloaded: number;
  total: number | null;
  lastChecked: number | null;
  error: string | null;
};
export default function AppUpdater({
  busy,
  activeTransfers,
  onBusyChange,
}: {
  busy: boolean;
  activeTransfers: boolean;
  onBusyChange: (busy: boolean) => void;
}) {
  const [status, setStatus] = useState<Status>({
    revision: -1,
    mode: "disabled",
    phase: "disabled",
    currentVersion: version,
    version: null,
    notes: null,
    downloaded: 0,
    total: null,
    lastChecked: null,
    error: null,
  });
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const modal = useRef<HTMLElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const installing = ["downloading", "installing"].includes(status.phase);
  const accept = (next: Status) =>
    setStatus((current) =>
      next.revision >= current.revision ? next : current,
    );
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    async function initialize() {
      const cleanup = await listen<Status>("app-update", (event) => {
        if (!disposed) accept(event.payload);
      });
      if (disposed) {
        cleanup();
        return;
      }
      unlisten = cleanup;
      const snapshot = await api<Status>("get_app_update_status");
      if (!disposed) accept(snapshot);
    }
    initialize().catch((e) => {
      if (!disposed) setError(String(e));
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  useEffect(() => {
    onBusyChange(installing);
    return () => onBusyChange(false);
  }, [installing, onBusyChange]);
  useEffect(() => {
    if (!open) return;
    modal.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => trigger.current?.focus();
  }, [open]);
  async function action(command: string) {
    setPending(true);
    setError("");
    try {
      const result = await api<Status | undefined>(command);
      if (result) accept(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setPending(false);
    }
  }
  return (
    <>
      <button
        ref={trigger}
        className={`update-trigger ${status.version ? "update-available" : ""}`}
        onClick={() => setOpen(true)}
        aria-label="检查应用更新"
      >
        S3 Studio <span className="version">v{status.currentVersion}</span>
        {status.version ? (
          <>
            <Sparkles size={12} /> 新版本
          </>
        ) : (
          <RefreshCw size={12} />
        )}
      </button>
      {open && (
        <div
          className="modal-backdrop"
          onClick={() => {
            if (!installing && !pending) setOpen(false);
          }}
        >
          <section
            ref={modal}
            className="modal update-modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="update-title"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Escape" && !installing && !pending) setOpen(false);
              if (e.key === "Tab") {
                const controls = Array.from(
                  modal.current?.querySelectorAll<HTMLElement>(
                    "button:not(:disabled), a[href]",
                  ) ?? [],
                );
                const first = controls[0],
                  last = controls.at(-1);
                if (!first) {
                  e.preventDefault();
                  return;
                }
                if (e.shiftKey && document.activeElement === first) {
                  e.preventDefault();
                  last?.focus();
                }
                if (!e.shiftKey && document.activeElement === last) {
                  e.preventDefault();
                  first.focus();
                }
              }
            }}
          >
            <button
              className="modal-close"
              disabled={installing || pending}
              aria-label="关闭更新窗口"
              onClick={() => setOpen(false)}
            >
              <X size={20} />
            </button>
            <div className="modal-symbol">
              <Sparkles size={26} />
            </div>
            <h2 id="update-title">
              {status.version ? `S3 Studio ${status.version}` : "软件更新"}
            </h2>
            <p className="modal-description">
              当前版本 {status.currentVersion}
              {status.version ? " · 有新版本可用" : ""}
            </p>
            <div aria-live="polite" className="update-status">
              {status.mode === "disabled" ? (
                <p>开发模式与网页预览不检查更新。安装发布版后即可使用。</p>
              ) : status.phase === "checking" ? (
                <p>正在检查新版本…</p>
              ) : status.phase === "installing" ? (
                <p>签名校验通过，正在安装更新…</p>
              ) : status.phase === "downloading" ? (
                <>
                  <p>正在下载并校验更新包…</p>
                  <progress
                    value={status.downloaded}
                    max={status.total || undefined}
                    aria-label="更新下载进度"
                  />
                  <p>
                    {formatBytes(status.downloaded)}
                    {status.total ? ` / ${formatBytes(status.total)}` : ""}
                  </p>
                </>
              ) : status.lastChecked &&
                !status.version &&
                !status.error &&
                !error ? (
                <p>已是最新版本。</p>
              ) : null}
              {status.lastChecked && (
                <small>
                  上次检查：
                  {new Date(status.lastChecked).toLocaleString("zh-CN")}
                </small>
              )}
            </div>
            {status.notes && <div className="update-notes">{status.notes}</div>}
            {(error || status.error) && (
              <p className="form-error" role="alert">
                {error || status.error}
              </p>
            )}
            {status.version && !installing && (
              <p className="modal-description">
                {status.mode === "inApp"
                  ? "确认后下载更新包，通过签名校验后安装并重启应用。"
                  : "当前安装方式需前往发布页下载新版本。账户配置与传输断点会保留。"}
              </p>
            )}
            {status.version && status.mode === "inApp" && activeTransfers && (
              <p className="form-error">请先暂停或完成所有传输，再安装更新。</p>
            )}
            <div className="modal-actions">
              <button
                className="secondary-button"
                disabled={
                  pending ||
                  installing ||
                  status.mode === "disabled" ||
                  status.phase === "checking"
                }
                onClick={() => void action("check_app_update")}
              >
                <RefreshCw size={15} /> 检查更新
              </button>
              {status.version && (
                <button
                  className="primary-button"
                  disabled={
                    pending ||
                    installing ||
                    busy ||
                    (status.mode === "inApp" && activeTransfers)
                  }
                  onClick={() =>
                    void action(
                      status.mode === "inApp"
                        ? "install_app_update"
                        : "open_update_download_page",
                    )
                  }
                >
                  <ArrowDownToLine size={15} />{" "}
                  {installing
                    ? "正在更新…"
                    : status.mode === "inApp"
                      ? "下载并安装"
                      : "打开下载页"}
                </button>
              )}
            </div>
          </section>
        </div>
      )}
    </>
  );
}
