import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowDownToLine,
  Check,
  CheckCircle2,
  ChevronDown,
  Info,
  LoaderCircle,
  Pause,
  Play,
  RotateCcw,
  Settings2,
  X,
} from "lucide-react";
import { api, native } from "./api";
import { formatBytes } from "./utils";
import type { Transfer, TransferSettings } from "./types";

export const activeTransfer = (t: Transfer) =>
  ["queued", "preparing", "running", "pausing", "finalizing"].includes(
    t.status,
  );
const labels: Record<Transfer["status"], string> = {
  queued: "排队中",
  preparing: "校验文件与断点",
  running: "传输中",
  pausing: "正在停止",
  paused: "已暂停",
  finalizing: "正在提交",
  done: "已完成",
  error: "失败，可重试",
  cancelled: "已取消",
};
export function useTransferManager() {
  const [transfers, setTransfers] = useState<Transfer[]>([]);
  const [settings, setSettings] = useState<TransferSettings>({
    concurrency: 8,
  });
  const [error, setError] = useState("");
  const [pending, setPending] = useState<Set<string>>(new Set());
  useEffect(() => {
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const changed = new Set<string>();
    async function initialize() {
      if (native) {
        const unlisten = await listen<Transfer>("transfer", (event) => {
          if (disposed) return;
          changed.add(event.payload.id);
          setTransfers((items) =>
            items.some((t) => t.id === event.payload.id)
              ? items.map((t) =>
                  t.id === event.payload.id ? event.payload : t,
                )
              : [event.payload, ...items],
          );
        });
        if (disposed) {
          unlisten();
          return;
        }
        cleanups.push(unlisten);
        const settingsListener = await listen<TransferSettings>(
          "transfer-settings",
          (event) => {
            if (!disposed) setSettings(event.payload);
          },
        );
        if (disposed) {
          settingsListener();
          return;
        }
        cleanups.push(settingsListener);
      }
      const [items, configuration] = await Promise.all([
        api<Transfer[]>("list_transfers"),
        api<TransferSettings>("get_transfer_settings"),
      ]);
      if (!disposed) {
        setTransfers((current) => [
          ...current.filter((t) => changed.has(t.id)),
          ...items.filter((t) => !changed.has(t.id)),
        ]);
        setSettings(configuration);
      }
    }
    initialize().catch((e) => {
      if (!disposed) setError(String(e));
    });
    return () => {
      disposed = true;
      cleanups.forEach((fn) => fn());
    };
  }, []);
  async function control(command: string, transferId: string) {
    setPending((items) => new Set([...items, transferId]));
    setError("");
    try {
      await api(command, { transferId });
      if (!native) setTransfers(await api<Transfer[]>("list_transfers"));
    } catch (e) {
      setError(String(e));
    } finally {
      setPending((items) => {
        const next = new Set(items);
        next.delete(transferId);
        return next;
      });
    }
  }
  async function saveSettings(concurrency: number) {
    setSettings(
      await api<TransferSettings>("set_transfer_settings", { concurrency }),
    );
  }
  async function clearFinished() {
    try {
      await api("clear_finished_transfers");
      setTransfers((items) =>
        items.filter((t) => !["done", "cancelled"].includes(t.status)),
      );
    } catch (e) {
      setError(String(e));
    }
  }
  return {
    transfers,
    settings,
    error,
    setError,
    pending,
    control,
    saveSettings,
    clearFinished,
  };
}
export type TransferManager = ReturnType<typeof useTransferManager>;
export default function TransfersPanel({
  manager,
  onSettings,
}: {
  manager: TransferManager;
  onSettings: () => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const [cancelTarget, setCancelTarget] = useState<Transfer | null>(null);
  const { transfers, settings, pending } = manager;
  const active = transfers.filter(activeTransfer).length;
  const paused = transfers.filter((t) => t.status === "paused").length;
  useEffect(() => {
    if (!cancelTarget) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") setCancelTarget(null);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [cancelTarget]);
  return (
    <section className="transfers-panel">
      <div className="transfers-head">
        <button
          className="transfers-heading"
          onClick={() => setExpanded(!expanded)}
        >
          <span>
            <ArrowDownToLine size={16} />
            传输活动{active > 0 && <small>{active}</small>}
          </span>
          <span className="muted">
            {active
              ? `${active} 项正在传输`
              : paused
                ? `${paused} 项已暂停`
                : "一切已就绪"}
            <span className="concurrency-badge">
              {settings.concurrency} 路并发
            </span>
            <ChevronDown size={15} className={expanded ? "" : "collapsed"} />
          </span>
        </button>
        <button
          className="transfer-settings-button"
          aria-label="传输设置"
          title="传输设置"
          onClick={onSettings}
        >
          <Settings2 size={16} />
        </button>
      </div>
      {manager.error && (
        <div className="error-banner" role="alert">
          <Info size={16} />
          <span>{manager.error}</span>
          <button
            aria-label="关闭传输错误"
            onClick={() => manager.setError("")}
          >
            <X size={15} />
          </button>
        </div>
      )}
      {expanded &&
        (transfers.length ? (
          <div className="transfers-list">
            {transfers.map((t) => (
              <div className="transfer-row" key={t.id} data-status={t.status}>
                {activeTransfer(t) ? (
                  <LoaderCircle size={17} className="spin" />
                ) : t.status === "done" ? (
                  <CheckCircle2 size={17} className="success" />
                ) : t.status === "paused" ? (
                  <Pause size={17} />
                ) : t.status === "cancelled" ? (
                  <X size={17} />
                ) : (
                  <Info size={17} className="danger-text" />
                )}
                <div className="transfer-details">
                  <strong>{t.name}</strong>
                  <span>
                    {t.direction === "upload" ? "上传" : "下载"} ·{" "}
                    {formatBytes(t.bytes)} / {formatBytes(t.total)}
                    {t.status === "running" && t.speed > 0
                      ? ` · ${formatBytes(t.speed)}/s`
                      : ""}
                  </span>
                  {t.error && <span className="transfer-error">{t.error}</span>}
                  <div
                    className="transfer-progress"
                    role="progressbar"
                    aria-label={`${t.name} 传输进度`}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={
                      t.total
                        ? Math.round((t.bytes / t.total) * 100)
                        : t.status === "done"
                          ? 100
                          : 0
                    }
                  >
                    <div
                      style={{
                        width: `${t.total ? Math.min(100, (t.bytes / t.total) * 100) : t.status === "done" ? 100 : 0}%`,
                      }}
                    />
                  </div>
                </div>
                <span className="transfer-status">
                  {labels[t.status]}
                  {t.status === "running" && t.total > 0 && (
                    <small>
                      {Math.min(100, Math.round((t.bytes / t.total) * 100))}%
                    </small>
                  )}
                </span>
                <div className="transfer-controls">
                  {["running", "preparing", "queued"].includes(t.status) && (
                    <button
                      disabled={pending.has(t.id)}
                      aria-label={`暂停 ${t.name}`}
                      title="暂停并保留断点"
                      onClick={() => manager.control("pause_transfer", t.id)}
                    >
                      <Pause size={16} />
                    </button>
                  )}
                  {["paused", "error"].includes(t.status) && (
                    <button
                      disabled={pending.has(t.id)}
                      aria-label={`${t.status === "error" ? "重试" : "继续"} ${t.name}`}
                      title="从断点继续"
                      onClick={() => manager.control("resume_transfer", t.id)}
                    >
                      {t.status === "error" ? (
                        <RotateCcw size={16} />
                      ) : (
                        <Play size={16} />
                      )}
                    </button>
                  )}
                  {!["done", "cancelled", "finalizing", "pausing"].includes(
                    t.status,
                  ) && (
                    <button
                      disabled={pending.has(t.id)}
                      aria-label={`取消 ${t.name}`}
                      title="取消并清理断点"
                      onClick={() => setCancelTarget(t)}
                    >
                      <X size={15} />
                    </button>
                  )}
                </div>
              </div>
            ))}
            {transfers.some((t) =>
              ["done", "cancelled"].includes(t.status),
            ) && (
              <button
                className="clear-transfers"
                onClick={manager.clearFinished}
              >
                清除已完成及已取消的记录
              </button>
            )}
          </div>
        ) : (
          <div className="transfers-empty">
            <div className="transfer-empty-icon">
              <Check size={18} />
            </div>
            <div>
              <strong>文件已就绪，随时开始。</strong>
              <span>支持并发传输、暂停和应用重启后继续。</span>
            </div>
            <span className="transfer-decoration">↑ ↓</span>
          </div>
        ))}
      {cancelTarget && (
        <div className="modal-backdrop" onClick={() => setCancelTarget(null)}>
          <section
            className="modal"
            role="dialog"
            aria-modal="true"
            aria-labelledby="cancel-transfer-title"
            onClick={(e) => e.stopPropagation()}
          >
            <h2 id="cancel-transfer-title">取消传输？</h2>
            <p className="modal-description">
              {cancelTarget.name}{" "}
              的断点将被清理；已经存在的源文件和目标文件不受影响。
            </p>
            <div className="modal-actions">
              <button
                className="secondary-button"
                autoFocus
                onClick={() => setCancelTarget(null)}
              >
                保留任务
              </button>
              <button
                className="danger-button"
                onClick={() => {
                  void manager.control("cancel_transfer", cancelTarget.id);
                  setCancelTarget(null);
                }}
              >
                取消并清理
              </button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
