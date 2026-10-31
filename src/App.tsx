import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown,
  ArrowDownToLine,
  ArrowUp,
  ArrowUpFromLine,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleHelp,
  Cloud,
  Copy,
  Database,
  File,
  FileArchive,
  FileCode2,
  FileImage,
  FileText,
  Folder,
  FolderPlus,
  HardDrive,
  Info,
  KeyRound,
  LayoutGrid,
  Link,
  List,
  LoaderCircle,
  MoreHorizontal,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  Trash2,
  X,
} from "lucide-react";
import { api, native } from "./api";
import TransfersPanel, {
  activeTransfer,
  useTransferManager,
} from "./TransfersPanel";
import AppUpdater from "./AppUpdater";
import { filterObjects, formatBytes, objectName } from "./utils";
import type {
  Account,
  AccountInput,
  Bucket,
  ObjectInfo,
  ObjectPage,
  S3Object,
} from "./types";

type Dialog =
  | { type: "account"; account?: Account }
  | {
      type:
        | "folder"
        | "bucket"
        | "copy"
        | "delete"
        | "upload"
        | "removeAccount"
        | "help"
        | "transferSettings";
    };
function FileIcon({ object, size = 20 }: { object: S3Object; size?: number }) {
  if (object.folder)
    return (
      <Folder
        size={size}
        fill="currentColor"
        strokeWidth={1.3}
        className="folder-icon"
      />
    );
  const ext = object.key.split(".").pop()?.toLowerCase();
  if (["png", "jpg", "jpeg", "webp", "svg"].includes(ext || ""))
    return <FileImage size={size} className="image-icon" />;
  if (["zip", "gz", "tar"].includes(ext || ""))
    return <FileArchive size={size} className="archive-icon" />;
  if (["json", "js", "ts", "html"].includes(ext || ""))
    return <FileCode2 size={size} className="code-icon" />;
  if (["pdf", "md", "txt"].includes(ext || ""))
    return <FileText size={size} className="text-icon" />;
  return <File size={size} className="muted" />;
}
const date = (value: string) =>
  value
    ? new Date(value).toLocaleString("zh-CN", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      })
    : "—";

export default function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [activeId, setActiveId] = useState("");
  const account = accounts.find((a) => a.id === activeId);
  const [buckets, setBuckets] = useState<Bucket[]>([]);
  const [bucket, setBucket] = useState("");
  const [prefix, setPrefix] = useState("");
  const [objects, setObjects] = useState<S3Object[]>([]);
  const [nextToken, setNextToken] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [bucketQuery, setBucketQuery] = useState("");
  const [sort, setSort] = useState<"name" | "size" | "modified">("name");
  const [ascending, setAscending] = useState(true);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [bucketLoading, setBucketLoading] = useState(false);
  const [operationBusy, setBusy] = useState(false);
  const [updateBusy, setUpdateBusy] = useState(false);
  const busy = operationBusy || updateBusy;
  const [error, setError] = useState("");
  const [toast, setToast] = useState("");
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [dialogError, setDialogError] = useState("");
  const [view, setView] = useState<"list" | "grid">("list");
  const [showInfo, setShowInfo] = useState(false);
  const [info, setInfo] = useState<ObjectInfo | null>(null);
  const [infoError, setInfoError] = useState("");
  const transferManager = useTransferManager();
  const transfers = transferManager.transfers;
  const refreshedUploads = useRef<Set<string>>(new Set());
  const searchRef = useRef<HTMLInputElement>(null);
  const bucketRequest = useRef(0);
  const objectRequest = useRef(0);
  const context = useRef({ activeId, bucket, prefix });
  context.current = { activeId, bucket, prefix };
  const visible = useMemo(
    () => filterObjects(objects, query, sort, ascending),
    [objects, query, sort, ascending],
  );
  const chosen = objects.filter((o) => selected.has(o.key));
  const first = chosen.length === 1 ? chosen[0] : undefined;
  const notify = (message: string) => setToast(message);
  const openDialog = (value: Dialog) => {
    setDialogError("");
    setDialog(value);
  };
  const loadAccounts = useCallback(async (id?: string) => {
    const items = await api<Account[]>("list_accounts");
    setAccounts(items);
    setActiveId(
      (current) =>
        id ||
        (items.some((a) => a.id === current) ? current : items[0]?.id || ""),
    );
  }, []);
  useEffect(() => {
    loadAccounts().catch((e) => setError(String(e)));
  }, [loadAccounts]);
  useEffect(() => {
    if (!toast) return;
    const timer = setTimeout(() => setToast(""), 4500);
    return () => clearTimeout(timer);
  }, [toast]);
  const loadBuckets = useCallback(async () => {
    const request = ++bucketRequest.current;
    setBuckets([]);
    setBucket("");
    setObjects([]);
    setNextToken(null);
    setSelected(new Set());
    setPrefix("");
    setError("");
    if (!activeId) {
      setBucketLoading(false);
      return;
    }
    setBucketLoading(true);
    try {
      const items = await api<Bucket[]>("list_buckets", {
        accountId: activeId,
      });
      if (request !== bucketRequest.current) return;
      setBuckets(items);
      setBucket(items[0]?.name || "");
    } catch (e) {
      if (request === bucketRequest.current) setError(String(e));
    } finally {
      if (request === bucketRequest.current) setBucketLoading(false);
    }
  }, [activeId]);
  useEffect(() => {
    void loadBuckets();
  }, [loadBuckets]);
  const loadObjects = useCallback(
    async (append = false) => {
      const request = ++objectRequest.current;
      const snapshot = { activeId, bucket, prefix };
      if (!append) {
        setObjects([]);
        setNextToken(null);
        setSelected(new Set());
      }
      if (!activeId || !bucket) {
        setLoading(false);
        return;
      }
      setLoading(true);
      setError("");
      try {
        const page = await api<ObjectPage>("list_objects", {
          accountId: activeId,
          bucket,
          prefix,
          token: append ? nextToken : null,
        });
        if (
          request !== objectRequest.current ||
          JSON.stringify(snapshot) !== JSON.stringify(context.current)
        )
          return;
        setObjects((items) =>
          append
            ? [
                ...items,
                ...page.objects.filter(
                  (o) => !items.some((i) => i.key === o.key),
                ),
              ]
            : page.objects,
        );
        setNextToken(page.nextToken);
      } catch (e) {
        if (
          request === objectRequest.current &&
          JSON.stringify(snapshot) === JSON.stringify(context.current)
        )
          setError(String(e));
      } finally {
        if (request === objectRequest.current) setLoading(false);
      }
    },
    [activeId, bucket, prefix, nextToken],
  );
  useEffect(() => {
    setQuery("");
    void loadObjects();
  }, [activeId, bucket, prefix]); // Pagination token must not trigger a fresh first page.
  useEffect(() => {
    const completed = transfers.filter(
      (t) =>
        t.direction === "upload" &&
        t.status === "done" &&
        !refreshedUploads.current.has(t.id),
    );
    completed.forEach((t) => refreshedUploads.current.add(t.id));
    if (completed.some((t) => t.accountId === activeId && t.bucket === bucket))
      void loadObjects();
  }, [transfers, activeId, bucket]);
  useEffect(() => {
    let cancelled = false;
    setInfo(null);
    setInfoError("");
    if (showInfo && first && !first.folder)
      api<ObjectInfo>("object_info", {
        accountId: activeId,
        bucket,
        key: first.key,
      })
        .then((data) => {
          if (!cancelled) setInfo(data);
        })
        .catch((e) => {
          if (!cancelled) setInfoError(String(e));
        });
    return () => {
      cancelled = true;
    };
  }, [showInfo, first?.key, activeId, bucket]);
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key === "f") {
        event.preventDefault();
        searchRef.current?.focus();
      }
      if (event.key === "Escape" && !busy) {
        setDialog(null);
        setSelected(new Set());
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [busy]);
  useEffect(() => {
    if (!dialog) return;
    const previous = document.activeElement as HTMLElement | null;
    const modal = document.querySelector<HTMLElement>('[role="dialog"]');
    const elements = () =>
      Array.from(
        modal?.querySelectorAll<HTMLElement>(
          "button:not(:disabled), input:not(:disabled)",
        ) || [],
      );
    if (!modal?.contains(document.activeElement)) elements()[0]?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = elements();
      const firstItem = items[0];
      const lastItem = items[items.length - 1];
      if (event.shiftKey && document.activeElement === firstItem) {
        event.preventDefault();
        lastItem?.focus();
      } else if (!event.shiftKey && document.activeElement === lastItem) {
        event.preventDefault();
        firstItem?.focus();
      }
    };
    document.addEventListener("keydown", trap);
    return () => {
      document.removeEventListener("keydown", trap);
      previous?.focus();
    };
  }, [dialog]);
  const args = { accountId: activeId, bucket };
  async function action(
    command: string,
    extra: Record<string, unknown> = {},
    message?: string,
  ) {
    setBusy(true);
    setError("");
    try {
      const result = await api<unknown>(command, { ...args, ...extra });
      if (message) notify(message);
      return result;
    } catch (e) {
      setError(String(e));
      return undefined;
    } finally {
      setBusy(false);
    }
  }
  async function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setDialogError("");
    const form = new FormData(event.currentTarget);
    try {
      const name = String(form.get("name") || "").trim();
      if (dialog?.type === "transferSettings") {
        await transferManager.saveSettings(Number(form.get("concurrency")));
        notify("设置保存成功");
      } else if (dialog?.type === "account") {
        const input: AccountInput = {
          id: dialog.account?.id || "",
          name,
          region: String(form.get("region")).trim(),
          endpoint: String(form.get("endpoint") || "").trim(),
          pathStyle: form.get("pathStyle") === "on",
          accessKey: String(form.get("accessKey") || "").trim(),
          secretKey: String(form.get("secretKey") || ""),
          sessionToken: String(form.get("sessionToken") || ""),
        };
        const saved = await api<Account>("save_account", { input });
        await loadAccounts(saved.id);
        notify("账户已安全保存");
      } else if (dialog?.type === "folder") {
        await api("create_folder", { ...args, key: prefix + name + "/" });
        await loadObjects();
        notify("文件夹已创建");
      } else if (dialog?.type === "bucket") {
        await api("create_bucket", { accountId: activeId, name });
        await loadBuckets();
        setBucket(name);
        notify("存储桶已创建");
      } else if (dialog?.type === "copy" && first) {
        await api("copy_object", {
          ...args,
          key: first.key,
          destination: name,
        });
        await loadObjects();
        notify("对象已复制");
      } else if (dialog?.type === "delete") {
        const count = await api<number>("delete_objects", {
          ...args,
          keys: chosen.filter((o) => !o.folder).map((o) => o.key),
          folders: chosen.filter((o) => o.folder).map((o) => o.key),
        });
        await loadObjects();
        notify(`已删除 ${count} 个对象`);
      } else if (dialog?.type === "removeAccount") {
        await api("remove_account", { accountId: activeId });
        await loadAccounts();
        notify("账户已移除");
      } else if (dialog?.type === "upload") {
        setDialog(null);
        const count = await api<number>("upload_files", { ...args, prefix });
        await loadObjects();
        if (count)
          notify(`已添加 ${count} 个上传任务，可在传输活动中暂停或继续`);
      }
      setDialog(null);
    } catch (e) {
      if (dialog?.type === "upload" || dialog?.type === "delete") {
        await loadObjects();
        if (dialog.type === "upload") setError(String(e));
        else setDialogError(String(e));
      } else setDialogError(String(e));
    } finally {
      setBusy(false);
    }
  }
  function toggle(key: string) {
    setSelected((items) => {
      const next = new Set(items);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });
  }
  function changeSort(value: typeof sort) {
    if (sort === value) setAscending(!ascending);
    else {
      setSort(value);
      setAscending(true);
    }
  }
  async function share() {
    if (!first) return;
    const url = await action("presign_object", {
      key: first.key,
      expires: 3600,
    });
    if (typeof url === "string") {
      try {
        await navigator.clipboard.writeText(url);
        notify("已复制分享链接，1 小时内有效");
      } catch {
        setError("无法访问剪贴板，请检查系统权限。");
      }
    }
  }
  const disabled = !bucket || loading || busy;
  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <img
              src="/brand/s3-studio-logo.svg"
              alt="S3 Studio Logo"
              width={48}
              height={48}
            />
          </div>
          <div>
            <strong>S3 Studio</strong>
            <span>云端文件，井然有序。</span>
          </div>
        </div>
        <div className="sidebar-heading">
          <span>工作空间</span>
          <button
            title="添加账户"
            aria-label="添加账户"
            onClick={() => openDialog({ type: "account" })}
          >
            <Plus size={16} />
          </button>
        </div>
        <div className="accounts-list">
          {accounts.map((a) => (
            <button
              className={`account-row ${activeId === a.id ? "active" : ""}`}
              key={a.id}
              onClick={() => {
                if (!busy) setActiveId(a.id);
              }}
              disabled={busy}
            >
              <div className={`provider-icon ${a.endpoint ? "purple" : ""}`}>
                {a.endpoint ? <Server size={18} /> : <Cloud size={18} />}
              </div>
              <div>
                <strong>{a.name}</strong>
                <span>{a.endpoint ? "S3 Compatible" : "Amazon S3"}</span>
              </div>
              {activeId === a.id && <span className="connection-dot" />}
            </button>
          ))}
        </div>
        <button
          className="add-account"
          onClick={() => openDialog({ type: "account" })}
        >
          <Plus size={15} /> 添加账户 <span>无限数量</span>
        </button>
        <div className="sidebar-divider" />
        <div className="sidebar-heading">
          <span>
            存储桶 <small>{buckets.length}</small>
          </span>
          <button
            title="创建存储桶"
            aria-label="创建存储桶"
            disabled={!account || busy}
            onClick={() => openDialog({ type: "bucket" })}
          >
            <Plus size={16} />
          </button>
        </div>
        {buckets.length > 0 && (
          <div className="bucket-search">
            <Search size={13} />
            <input
              aria-label="搜索存储桶"
              placeholder="查找存储桶"
              value={bucketQuery}
              onChange={(e) => setBucketQuery(e.target.value)}
            />
          </div>
        )}
        <div className="bucket-list">
          {bucketLoading ? (
            <div className="sidebar-empty">
              <LoaderCircle size={18} className="spin" /> 正在连接…
            </div>
          ) : (
            buckets
              .filter((b) => b.name.includes(bucketQuery))
              .map((b) => (
                <button
                  key={b.name}
                  className={`bucket-row ${bucket === b.name ? "active" : ""}`}
                  onClick={() => {
                    setBucket(b.name);
                    setPrefix("");
                  }}
                >
                  <Database size={16} />
                  <span>{b.name}</span>
                  {bucket === b.name && <ChevronRight size={13} />}
                </button>
              ))
          )}
          {!bucketLoading && !buckets.length && (
            <div className="sidebar-empty">
              {account ? "暂无存储桶" : "添加账户以开始"}
            </div>
          )}
        </div>
        <div className="sidebar-bottom">
          <div className="security-note">
            <ShieldCheck size={17} />
            <div>
              <strong>只属于你的工作空间</strong>
              <span>凭据保存在系统密钥库</span>
            </div>
          </div>
          <div className="bottom-links">
            <button
              disabled={!account || busy}
              onClick={() => openDialog({ type: "account", account })}
            >
              <Settings2 size={16} /> 账户设置
            </button>
            <button
              title="使用帮助"
              aria-label="使用帮助"
              onClick={() => openDialog({ type: "help" })}
            >
              <CircleHelp size={17} />
            </button>
          </div>
        </div>
      </aside>
      <main className="workspace">
        <header className="topbar">
          <div className="workspace-label">
            <span className="live-dot" />
            {account?.name || "你的工作空间"}
            <ChevronDown size={13} />
          </div>
          <div className="topbar-right">
            <span className="platform-tag">macOS · Windows · Linux</span>
            <span className="avatar">
              {account?.name.slice(0, 1).toUpperCase() || "S"}
            </span>
          </div>
        </header>
        {!native && (
          <div className="preview-banner">
            <Info size={14} />
            <span>
              交互预览 · 当前展示示例数据，桌面应用支持连接真实 S3 账户。
            </span>
            <span className="preview-badge">PREVIEW</span>
          </div>
        )}
        <section className="workspace-body">
          <div className="page-heading">
            <div>
              <div className="eyebrow">OBJECT STORAGE</div>
              <h1>{bucket || "欢迎来到 S3 Studio"}</h1>
              <p>
                {bucket
                  ? "让每一份文件，都有自己的位置。"
                  : "连接你的云存储，从一个清晰的工作空间开始。"}
              </p>
            </div>
            <button
              className="primary-button"
              disabled={disabled}
              onClick={() => openDialog({ type: "upload" })}
            >
              <ArrowUpFromLine size={17} /> 上传文件{" "}
              <span className="button-plus">+</span>
            </button>
          </div>
          <div className="bucket-summary">
            <div>
              <Database size={16} />
              <span>{bucket || "尚未选择存储桶"}</span>
            </div>
            <span className="summary-divider" />
            <span>
              <span className="live-dot" />
              {account?.region || "未连接"}
            </span>
            <span className="summary-divider" />
            <span>{native ? "安全直连" : "示例工作空间"}</span>
            <div className="summary-spacer" />
            <span className="subtle">
              {objects.filter((o) => !o.folder).length} 个文件
              {nextToken ? "（已加载）" : ""}
            </span>
          </div>
          <div className="file-panel">
            <div className="file-toolbar">
              <nav className="breadcrumbs" aria-label="当前位置">
                <button disabled={!bucket} onClick={() => setPrefix("")}>
                  <HardDrive size={16} /> 根目录
                </button>
                {prefix
                  .split("/")
                  .filter(Boolean)
                  .map((part, index, parts) => (
                    <span key={index}>
                      <ChevronRight size={13} />
                      <button
                        onClick={() =>
                          setPrefix(parts.slice(0, index + 1).join("/") + "/")
                        }
                      >
                        {part}
                      </button>
                    </span>
                  ))}
              </nav>
              <div className="toolbar-actions">
                <button
                  className="icon-button"
                  title="新建文件夹"
                  aria-label="新建文件夹"
                  disabled={disabled}
                  onClick={() => openDialog({ type: "folder" })}
                >
                  <FolderPlus size={18} />
                </button>
                <button
                  className="icon-button"
                  title="刷新"
                  aria-label="刷新"
                  disabled={disabled}
                  onClick={() => loadObjects()}
                >
                  <RefreshCw size={17} className={loading ? "spin" : ""} />
                </button>
                <span className="toolbar-divider" />
                <div className="view-switch">
                  <button
                    className={view === "list" ? "selected" : ""}
                    title="列表视图"
                    aria-label="列表视图"
                    onClick={() => setView("list")}
                  >
                    <List size={17} />
                  </button>
                  <button
                    className={view === "grid" ? "selected" : ""}
                    title="网格视图"
                    aria-label="网格视图"
                    onClick={() => setView("grid")}
                  >
                    <LayoutGrid size={15} />
                  </button>
                </div>
                <button
                  className={`icon-button ${showInfo ? "selected" : ""}`}
                  title="对象详情"
                  aria-label="对象详情"
                  onClick={() => setShowInfo(!showInfo)}
                >
                  <Info size={17} />
                </button>
              </div>
            </div>
            <div className="search-row">
              <div className="object-search">
                <Search size={17} />
                <input
                  ref={searchRef}
                  aria-label="搜索当前目录"
                  placeholder="搜索当前目录中的文件…"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                {query ? (
                  <button aria-label="清除搜索" onClick={() => setQuery("")}>
                    <X size={14} />
                  </button>
                ) : (
                  <kbd>⌘ / Ctrl F</kbd>
                )}
              </div>
              {selected.size > 0 ? (
                <div className="selection-actions">
                  <span>已选 {selected.size} 项</span>
                  <button
                    disabled={busy || !first || first.folder}
                    title="下载"
                    aria-label="下载所选文件"
                    onClick={() =>
                      action("download_object", { key: first!.key }, undefined)
                    }
                  >
                    <ArrowDownToLine size={17} />
                  </button>
                  <button
                    disabled={busy || !first || first.folder}
                    title="复制对象"
                    aria-label="复制所选对象"
                    onClick={() => openDialog({ type: "copy" })}
                  >
                    <Copy size={16} />
                  </button>
                  <button
                    disabled={busy || !first || first.folder}
                    title="分享链接"
                    aria-label="复制分享链接"
                    onClick={share}
                  >
                    <Link size={16} />
                  </button>
                  <button
                    className="danger-text"
                    disabled={busy}
                    title="删除"
                    aria-label="删除所选项目"
                    onClick={() => openDialog({ type: "delete" })}
                  >
                    <Trash2 size={16} />
                  </button>
                </div>
              ) : (
                <span className="search-hint">双击文件夹进入</span>
              )}
            </div>
            {error && (
              <div className="error-banner" role="alert">
                <Info size={17} />
                <span>{error}</span>
                <button aria-label="关闭错误" onClick={() => setError("")}>
                  <X size={16} />
                </button>
              </div>
            )}
            <div className="content-area">
              <div className="objects-area">
                {loading && objects.length === 0 ? (
                  <div className="empty-state">
                    <LoaderCircle size={30} className="spin" />
                    <h3>正在加载文件</h3>
                    <p>正在安全连接你的存储空间…</p>
                  </div>
                ) : !visible.length ? (
                  <div className="empty-state">
                    <div className="empty-icon">
                      <Cloud size={36} />
                    </div>
                    <h3>
                      {query
                        ? "没有找到匹配的文件"
                        : !account
                          ? "一切从连接开始"
                          : !bucket
                            ? "创建你的第一个存储桶"
                            : "这里还没有文件"}
                    </h3>
                    <p>
                      {query
                        ? "试试其他关键词。"
                        : !account
                          ? "支持 AWS、MinIO 和其他 S3 兼容服务，账户数量不限。"
                          : "添加文件，为你的工作腾出更多空间。"}
                    </p>
                    {!account && (
                      <button
                        className="primary-button"
                        onClick={() => openDialog({ type: "account" })}
                      >
                        <Plus size={16} /> 连接第一个账户
                      </button>
                    )}
                  </div>
                ) : view === "list" ? (
                  <table className="object-table">
                    <thead>
                      <tr>
                        <th className="checkbox-cell">
                          <input
                            type="checkbox"
                            aria-label="选择当前可见对象"
                            checked={
                              visible.length > 0 &&
                              visible.every((o) => selected.has(o.key))
                            }
                            onChange={(e) =>
                              setSelected(
                                e.target.checked
                                  ? new Set(visible.map((o) => o.key))
                                  : new Set(),
                              )
                            }
                          />
                        </th>
                        <th>
                          <button onClick={() => changeSort("name")}>
                            名称{" "}
                            {sort === "name" &&
                              (ascending ? (
                                <ArrowUp size={12} />
                              ) : (
                                <ArrowDown size={12} />
                              ))}
                          </button>
                        </th>
                        <th>
                          <button onClick={() => changeSort("size")}>
                            大小{" "}
                            {sort === "size" &&
                              (ascending ? (
                                <ArrowUp size={12} />
                              ) : (
                                <ArrowDown size={12} />
                              ))}
                          </button>
                        </th>
                        <th>
                          <button onClick={() => changeSort("modified")}>
                            修改时间{" "}
                            {sort === "modified" &&
                              (ascending ? (
                                <ArrowUp size={12} />
                              ) : (
                                <ArrowDown size={12} />
                              ))}
                          </button>
                        </th>
                        <th className="storage-column">存储类型</th>
                        <th />
                      </tr>
                    </thead>
                    <tbody>
                      {visible.map((object) => (
                        <tr
                          key={object.key}
                          className={
                            selected.has(object.key) ? "row-selected" : ""
                          }
                          onClick={() => toggle(object.key)}
                          onDoubleClick={() => {
                            if (object.folder) setPrefix(object.key);
                            else {
                              setSelected(new Set([object.key]));
                              setShowInfo(true);
                            }
                          }}
                        >
                          <td className="checkbox-cell">
                            <input
                              type="checkbox"
                              aria-label={`选择 ${objectName(object.key, prefix)}`}
                              checked={selected.has(object.key)}
                              onClick={(e) => e.stopPropagation()}
                              onChange={() => toggle(object.key)}
                            />
                          </td>
                          <td>
                            <div className="filename">
                              <FileIcon object={object} />
                              <span>{objectName(object.key, prefix)}</span>
                            </div>
                          </td>
                          <td className="muted">
                            {object.folder ? "—" : formatBytes(object.size)}
                          </td>
                          <td className="muted">
                            {object.folder ? "—" : date(object.modified)}
                          </td>
                          <td className="storage-column">
                            <span
                              className={`storage-badge ${object.folder ? "folder-badge" : ""}`}
                            >
                              {object.folder
                                ? "文件夹"
                                : object.storageClass || "STANDARD"}
                            </span>
                          </td>
                          <td>
                            <button
                              className="row-more"
                              title="查看详情"
                              aria-label={`查看 ${objectName(object.key, prefix)}`}
                              onClick={(e) => {
                                e.stopPropagation();
                                setSelected(new Set([object.key]));
                                setShowInfo(true);
                              }}
                            >
                              <MoreHorizontal size={17} />
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <div className="object-grid">
                    {visible.map((object) => (
                      <button
                        key={object.key}
                        className={`object-card ${selected.has(object.key) ? "selected" : ""}`}
                        onClick={() => toggle(object.key)}
                        onDoubleClick={() =>
                          object.folder
                            ? setPrefix(object.key)
                            : setShowInfo(true)
                        }
                      >
                        <div className="card-icon">
                          <FileIcon object={object} size={42} />
                        </div>
                        <strong>{objectName(object.key, prefix)}</strong>
                        <span>
                          {object.folder ? "文件夹" : formatBytes(object.size)}
                        </span>
                        {selected.has(object.key) && (
                          <CheckCircle2 className="card-check" size={16} />
                        )}
                      </button>
                    ))}
                  </div>
                )}
                {nextToken && (
                  <div className="load-more">
                    <button
                      disabled={loading}
                      onClick={() => loadObjects(true)}
                    >
                      {loading ? "加载中…" : "加载更多对象"}
                    </button>
                    <span>搜索和排序适用于已加载的对象</span>
                  </div>
                )}
              </div>
              {showInfo && (
                <aside className="inspector">
                  <div className="inspector-heading">
                    对象详情
                    <button
                      aria-label="关闭详情"
                      onClick={() => setShowInfo(false)}
                    >
                      <X size={16} />
                    </button>
                  </div>
                  {first ? (
                    <>
                      <div className="inspector-file">
                        <FileIcon object={first} size={44} />
                        <strong>{objectName(first.key, prefix)}</strong>
                      </div>
                      <dl>
                        <dt>对象键</dt>
                        <dd>{first.key}</dd>
                        <dt>大小</dt>
                        <dd>
                          {first.folder
                            ? "文件夹"
                            : formatBytes(info?.size ?? first.size)}
                        </dd>
                        <dt>最后修改</dt>
                        <dd>{date(info?.modified || first.modified)}</dd>
                        <dt>存储类型</dt>
                        <dd>{first.storageClass || "—"}</dd>
                        {info && (
                          <>
                            <dt>Content-Type</dt>
                            <dd>{info.contentType}</dd>
                            <dt>ETag</dt>
                            <dd>{info.etag}</dd>
                            {Object.entries(info.metadata).map(([k, v]) => (
                              <div key={k}>
                                <dt>{k}</dt>
                                <dd>{v}</dd>
                              </div>
                            ))}
                          </>
                        )}
                      </dl>
                      {infoError && <p className="danger-text">{infoError}</p>}
                      {!first.folder && (
                        <button
                          className="secondary-button full-width"
                          disabled={busy}
                          onClick={() =>
                            action("download_object", { key: first.key })
                          }
                        >
                          <ArrowDownToLine size={16} /> 下载文件
                        </button>
                      )}
                    </>
                  ) : (
                    <p className="muted">选择一个对象以查看详情。</p>
                  )}
                </aside>
              )}
            </div>
            <div className="table-footer">
              <span>
                {visible.length} 个项目{query && " · 搜索结果"}
                {selected.size > 0 && ` · 已选择 ${selected.size} 项`}
              </span>
              <span>
                {formatBytes(objects.reduce((total, o) => total + o.size, 0))} ·
                当前已加载
              </span>
            </div>
          </div>
          <TransfersPanel
            manager={transferManager}
            onSettings={() => openDialog({ type: "transferSettings" })}
          />
          <footer className="workspace-footer">
            <span>
              <ShieldCheck size={13} /> 本地凭据 · 直接连接 · 不限账户
            </span>
            <AppUpdater
              busy={busy}
              activeTransfers={transferManager.transfers.some(activeTransfer)}
              onBusyChange={setUpdateBusy}
            />
          </footer>
        </section>
      </main>
      {toast && (
        <div className="toast" role="status">
          <CheckCircle2 size={18} />
          {toast}
        </div>
      )}
      {dialog && (
        <div
          className="modal-backdrop"
          onClick={() => {
            if (!busy) setDialog(null);
          }}
        >
          <section
            className={`modal ${dialog.type === "account" ? "account-modal" : ""}`}
            role="dialog"
            aria-modal="true"
            aria-labelledby="dialog-title"
            onClick={(e) => e.stopPropagation()}
          >
            <button
              className="modal-close"
              disabled={busy}
              aria-label="关闭弹窗"
              onClick={() => setDialog(null)}
            >
              <X size={20} />
            </button>
            <div className="modal-symbol">
              {dialog.type === "account" ? (
                <KeyRound size={26} />
              ) : dialog.type === "delete" ? (
                <Trash2 size={26} />
              ) : (
                <Cloud size={26} />
              )}
            </div>
            <h2 id="dialog-title">
              {
                {
                  account:
                    dialog.type === "account" && dialog.account
                      ? "账户设置"
                      : "连接一个新账户",
                  folder: "新建文件夹",
                  bucket: "创建存储桶",
                  copy: "复制对象",
                  delete: "删除所选项目？",
                  upload: "上传文件",
                  removeAccount: "移除这个账户？",
                  help: "为云端工作而设计",
                  transferSettings: "传输设置",
                }[dialog.type]
              }
            </h2>
            <p className="modal-description">
              {
                {
                  account: "所有凭据仅保存于本机系统密钥库，账户数量没有限制。",
                  folder: "在当前目录中创建一个新的文件夹。",
                  bucket: "在当前账户和区域中创建新的 S3 存储桶。",
                  copy: "输入目标对象的完整路径，复制到当前存储桶。",
                  delete: chosen.some((o) => o.folder)
                    ? `将递归删除 ${chosen.filter((o) => o.folder).length} 个目录及其中所有文件和子目录，并删除 ${chosen.filter((o) => !o.folder).length} 个所选文件。未启用版本控制的对象无法恢复。`
                    : `即将删除 ${chosen.length} 个文件。未启用版本控制的对象无法恢复。`,
                  upload:
                    "接下来选择本地文件。当前目录中同名的对象将被覆盖；大文件自动分片上传。",
                  removeAccount:
                    "删除本机保存的账户配置和凭据，云端文件不受影响。",
                  help: "一个基于 Tauri 的跨平台 S3 工作空间。",
                  transferSettings:
                    "设置整个应用的并发上限，所有上传和下载任务共同使用。建议从默认 8 路开始。",
                }[dialog.type]
              }
            </p>
            {dialog.type === "help" ? (
              <div className="help-content">
                <p>
                  ① 添加账户：填写 Access Key、Secret Key 和 Region。MinIO
                  等服务可以配置 Endpoint 和路径风格。
                </p>
                <p>
                  ②
                  选择存储桶，双击文件夹浏览。选择文件后可下载、复制、分享或删除。
                </p>
                <p>③ 使用 ⌘ / Ctrl + F 搜索，Escape 关闭弹窗或取消选择。</p>
                <p>
                  删除目录会同时删除其中所有文件和子目录。分享链接有效期 1
                  小时。传输活动支持暂停、继续和失败重试；关闭应用后，再次打开可从断点继续。
                </p>
                <p>
                  首次使用需具备相应的 S3 权限；Ubuntu 需启动支持 Secret Service
                  的密钥库。
                </p>
                <button
                  className="primary-button full-width"
                  onClick={() => setDialog(null)}
                >
                  开始使用
                </button>
              </div>
            ) : (
              <form onSubmit={submit}>
                {dialog.type === "transferSettings" && (
                  <>
                    <label>
                      并发数量
                      <input
                        name="concurrency"
                        aria-label="并发数量"
                        type="number"
                        min={1}
                        max={32}
                        step={1}
                        defaultValue={transferManager.settings.concurrency}
                        required
                        autoFocus
                      />
                    </label>
                    <p className="modal-description">
                      支持 1–32
                      路。设置自动保存；降低上限时，已开始的分片会先完成，后续分片按新上限执行。
                    </p>
                  </>
                )}
                {dialog.type === "account" && (
                  <>
                    <label>
                      账户名称
                      <input
                        name="name"
                        defaultValue={dialog.account?.name}
                        placeholder="例如：生产环境 / 个人存储"
                        required
                        autoFocus
                        maxLength={80}
                      />
                    </label>
                    <div className="form-grid">
                      <label>
                        Access Key ID
                        <input
                          name="accessKey"
                          autoComplete="off"
                          placeholder={
                            dialog.account ? "留空保留现有凭据" : "AKIA…"
                          }
                          required={!dialog.account}
                        />
                      </label>
                      <label>
                        Region
                        <input
                          name="region"
                          defaultValue={dialog.account?.region || "us-east-1"}
                          required
                          placeholder="us-east-1"
                        />
                      </label>
                    </div>
                    <label>
                      Secret Access Key
                      <input
                        name="secretKey"
                        type="password"
                        autoComplete="new-password"
                        placeholder={
                          dialog.account
                            ? "留空保留现有凭据"
                            : "你的 Secret Access Key"
                        }
                        required={!dialog.account}
                      />
                    </label>
                    <label>
                      Session Token <span>可选 · 临时凭据</span>
                      <input
                        name="sessionToken"
                        type="password"
                        autoComplete="new-password"
                        placeholder="使用临时凭据时填写；更新凭据时留空可清除"
                      />
                    </label>
                    <label>
                      Endpoint <span>可选 · S3 兼容服务</span>
                      <input
                        name="endpoint"
                        type="url"
                        defaultValue={dialog.account?.endpoint}
                        placeholder="https://s3.example.com"
                      />
                    </label>
                    <label className="checkbox-label">
                      <input
                        type="checkbox"
                        name="pathStyle"
                        defaultChecked={dialog.account?.pathStyle ?? true}
                      />{" "}
                      使用路径风格访问 <span>MinIO 等服务推荐开启</span>
                    </label>
                    {dialog.account && (
                      <button
                        className="remove-account"
                        type="button"
                        disabled={busy}
                        onClick={() => openDialog({ type: "removeAccount" })}
                      >
                        <Trash2 size={14} /> 移除账户
                      </button>
                    )}
                  </>
                )}
                {["folder", "bucket", "copy"].includes(dialog.type) && (
                  <label>
                    {dialog.type === "copy"
                      ? "目标对象键"
                      : dialog.type === "bucket"
                        ? "存储桶名称"
                        : "文件夹名称"}
                    <input
                      name="name"
                      autoFocus
                      required
                      placeholder={
                        dialog.type === "bucket"
                          ? "my-new-bucket"
                          : dialog.type === "copy"
                            ? prefix +
                              "copy-" +
                              objectName(first?.key || "", prefix)
                            : "新文件夹"
                      }
                      defaultValue={
                        dialog.type === "copy"
                          ? prefix +
                            "copy-" +
                            objectName(first?.key || "", prefix)
                          : ""
                      }
                      pattern={
                        dialog.type === "bucket"
                          ? "[a-z0-9][a-z0-9.\-]{1,61}[a-z0-9]"
                          : dialog.type === "folder"
                            ? "[^/\\]+"
                            : undefined
                      }
                    />
                  </label>
                )}
                {dialog.type === "delete" && (
                  <div className="delete-list">
                    {chosen.map((o) => (
                      <div key={o.key}>
                        <FileIcon object={o} />
                        {objectName(o.key, prefix)}
                      </div>
                    ))}
                  </div>
                )}
                {dialogError && (
                  <div className="form-error" role="alert">
                    {dialogError}
                  </div>
                )}
                <div className="modal-actions">
                  <button
                    className="secondary-button"
                    type="button"
                    disabled={busy}
                    onClick={() => setDialog(null)}
                  >
                    取消
                  </button>
                  <button
                    type="submit"
                    disabled={busy}
                    className={
                      ["delete", "removeAccount"].includes(dialog.type)
                        ? "danger-button"
                        : "primary-button"
                    }
                  >
                    {busy && <LoaderCircle size={16} className="spin" />}
                    {busy
                      ? "处理中…"
                      : {
                          account: "保存账户",
                          folder: "创建文件夹",
                          bucket: "创建存储桶",
                          copy: "复制",
                          delete: "确认删除",
                          upload: "选择文件并上传",
                          removeAccount: "移除账户",
                          transferSettings: "保存设置",
                        }[dialog.type]}
                  </button>
                </div>
              </form>
            )}
          </section>
        </div>
      )}
    </div>
  );
}
