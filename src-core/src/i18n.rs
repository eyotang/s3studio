use std::sync::atomic::{AtomicBool, Ordering};
static ENGLISH: AtomicBool = AtomicBool::new(false);
pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}
pub fn t(text: &str) -> &str {
    translate(text, english())
}
fn translate(text: &str, english: bool) -> &str {
    if !english {
        return text;
    }
    match text {
        "设置…" => "Settings…",
        "设置" => "Settings",
        "主题" => "Theme",
        "亮色" => "Light",
        "暗色" => "Dark",
        "语言和主题立即生效并自动保存。" => "Language and theme apply immediately and are saved automatically.",
        "更新版本号无效。" => "Invalid update version.",
        "仅接受正式版本更新。" => "Only stable releases are supported.",
        "更新清单过大。" => "Update manifest is too large.",
        "更新清单格式无效。" => "Invalid update manifest.",
        "更新说明过长。" => "Release notes are too long.",
        "更新操作正在进行中。" => "An update is already in progress.",
        "更新包地址不属于官方发布仓库。" => {
            "The update URL is outside the official release repository."
        }
        "当前安装方式需从发布页下载更新。" => {
            "Download updates from the releases page for this installation."
        }
        "请先检查可用更新。" => "Check for updates first.",
        "开发模式与网页预览不检查更新。安装发布版后即可使用。" => {
            "Updates are unavailable in development and preview mode. Install a release build to enable updates."
        }
        "无法定位应用数据目录" => "Unable to locate application data directory",
        "当前是界面预览。请在桌面应用中连接真实账户。" => {
            "This is a preview. Connect a real account in the desktop app."
        }
        "搜索当前目录中的文件…" => "Search files in this folder…",
        "查找存储桶" => "Find a bucket",
        "例如：生产环境 / 个人存储" => "For example: Production / Personal",
        "留空保留现有凭据" => "Leave blank to keep existing credentials",
        "你的 Secret Access Key" => "Your Secret Access Key",
        "使用临时凭据时填写；更新凭据时留空可清除" => {
            "For temporary credentials; leave blank when updating to clear"
        }
        "新文件夹" => "New folder",
        "请填写此字段。" => "This field is required.",
        "请填写完整的账户名称、Access Key、Secret Key 和 Region。" => {
            "Enter an account name, Access Key, Secret Key, and Region."
        }
        "文件夹名称不能包含 / 或 \\。" => "Folder names cannot contain / or \\\\.",
        "并发数量需为 1–32 的整数。" => "Concurrency must be an integer from 1 to 32.",
        "账户已安全保存" => "Account saved securely",
        "账户已移除" => "Account removed",
        "存储桶已创建" => "Bucket created",
        "设置保存成功" => "Settings saved successfully",
        "文件夹已创建" => "Folder created",
        "对象已复制" => "Object copied",
        "已复制分享链接，1 小时内有效" => "Share link copied; valid for 1 hour",
        "关于 S3 Studio" => "About S3 Studio",
        "检查更新…" => "Check for Updates…",
        "账户设置" => "Account Settings",
        "连接一个新账户" => "Connect an Account",
        "新建文件夹" => "New Folder",
        "创建存储桶" => "Create Bucket",
        "复制对象" => "Copy Object",
        "删除所选项目？" => "Delete selected items?",
        "上传文件" => "Upload Files",
        "移除这个账户？" => "Remove this account?",
        "为云端工作而设计" => "Designed for cloud work",
        "传输设置" => "Transfer Settings",
        "取消传输？" => "Cancel transfer?",
        "软件更新" => "Software Update",
        "保存账户" => "Save Account",
        "创建文件夹" => "Create Folder",
        "复制" => "Copy",
        "确认删除" => "Delete",
        "选择文件并上传" => "Select Files and Upload",
        "移除账户" => "Remove Account",
        "保存设置" => "Save Settings",
        "开始使用" => "Get Started",
        "关闭" => "Close",
        "取消并清理" => "Cancel and Clean Up",
        "检查更新" => "Check for Updates",
        "排队中" => "Queued",
        "校验文件与断点" => "Verifying files and checkpoints",
        "传输中" => "Transferring",
        "正在停止" => "Stopping",
        "已暂停" => "Paused",
        "正在提交" => "Finalizing",
        "已完成" => "Completed",
        "失败，可重试" => "Failed; retry available",
        "已取消" => "Cancelled",
        "传输活动" => "Transfers",
        "一切已就绪" => "All ready",
        "关闭传输错误" => "Dismiss transfer error",
        "文件已就绪，随时开始。" => "Your files are ready.",
        "支持并发传输、暂停和应用重启后继续。" => {
            "Concurrent transfers with pause and resume across app restarts."
        }
        "重试" => "Retry",
        "继续" => "Resume",
        "上传" => "Upload",
        "下载" => "Download",
        "清除已完成及已取消的记录" => "Clear completed and cancelled transfers",
        "云端文件，井然有序。" => "Cloud files, beautifully organized.",
        "工作空间" => "Workspace",
        "添加账户" => "Add Account",
        "无限数量" => "Unlimited",
        "存储桶" => "Buckets",
        "搜索存储桶" => "Search buckets",
        "正在连接…" => "Connecting…",
        "暂无存储桶" => "No buckets yet",
        "添加账户以开始" => "Add an account to get started",
        "只属于你的工作空间" => "A workspace of your own",
        "凭据保存在系统密钥库" => "Credentials stored in the system keychain",
        "使用帮助" => "Help",
        "你的工作空间" => "Your workspace",
        "交互预览 · 当前展示示例数据，桌面应用支持连接真实 S3 账户。" => {
            "Interactive preview · Showing sample data. Connect real S3 accounts in the desktop app."
        }
        "欢迎来到 S3 Studio" => "Welcome to S3 Studio",
        "连接你的云存储，从一个清晰的工作空间开始。" => {
            "Connect your cloud storage in one clear workspace."
        }
        "让每一份文件，都有自己的位置。" => "A place for every file.",
        "尚未选择存储桶" => "No bucket selected",
        "未连接" => "Disconnected",
        "示例工作空间" => "Sample workspace",
        "安全直连" => "Secure connection",
        "（已加载）" => " (loaded)",
        "本地凭据 · 直接连接 · 不限账户" => {
            "Local credentials · Direct connection · Unlimited accounts"
        }
        "检查应用更新" => "Check for app updates",
        "新版本" => "Update available",
        "根目录" => "Root",
        "刷新" => "Refresh",
        "列表视图" => "List view",
        "网格视图" => "Grid view",
        "对象详情" => "Object details",
        "搜索当前目录" => "Search this folder",
        "清除搜索" => "Clear search",
        "双击文件夹进入" => "Double-click a folder to open",
        "下载所选文件" => "Download selected files",
        "复制所选对象" => "Copy selected object",
        "复制分享链接" => "Copy share link",
        "删除所选项目" => "Delete selected items",
        "关闭错误" => "Dismiss error",
        "正在加载文件" => "Loading files",
        "没有找到匹配的文件" => "No matching files",
        "一切从连接开始" => "Start with a connection",
        "创建你的第一个存储桶" => "Create your first bucket",
        "这里还没有文件" => "No files here yet",
        "正在安全连接你的存储空间…" => "Connecting securely to your storage…",
        "试试其他关键词。" => "Try another search.",
        "支持 AWS、MinIO 和其他 S3 兼容服务，账户数量不限。" => {
            "Supports AWS, MinIO, and other S3-compatible services. Unlimited accounts."
        }
        "添加文件，为你的工作腾出更多空间。" => {
            "Add files and make room for your work."
        }
        "连接第一个账户" => "Connect Your First Account",
        "加载中…" => "Loading…",
        "加载更多对象" => "Load More Objects",
        "搜索和排序适用于已加载的对象" => {
            "Search and sorting apply to loaded objects"
        }
        " · 搜索结果" => " · Search results",
        "选择当前可见对象" => "Select visible objects",
        "名称" => "Name",
        "大小" => "Size",
        "修改时间" => "Modified",
        "存储类型" => "Storage class",
        "文件夹" => "Folder",
        "关闭详情" => "Close details",
        "对象键" => "Object key",
        "最后修改" => "Last modified",
        "下载文件" => "Download File",
        "选择一个对象以查看详情。" => "Select an object to view its details.",
        "所有凭据仅保存于本机系统密钥库，账户数量没有限制。" => {
            "Credentials stay in your system keychain. There is no account limit."
        }
        "在当前目录中创建一个新的文件夹。" => {
            "Create a new folder in the current directory."
        }
        "在当前账户和区域中创建新的 S3 存储桶。" => {
            "Create an S3 bucket in the current account and region."
        }
        "输入目标对象的完整路径，复制到当前存储桶。" => {
            "Enter the full destination key to copy within this bucket."
        }
        "接下来选择本地文件。当前目录中同名的对象将被覆盖；大文件自动分片上传。" => {
            "Select local files next. Objects with matching names will be overwritten; large files use multipart uploads."
        }
        "删除本机保存的账户配置和凭据，云端文件不受影响。" => {
            "Remove locally saved account settings and credentials. Cloud files are unaffected."
        }
        "一个基于 Tauri 的跨平台 S3 工作空间。" => {
            "A cross-platform S3 workspace built with GPUI."
        }
        "设置整个应用的并发上限，所有上传和下载任务共同使用。建议从默认 8 路开始。" => {
            "Set the shared concurrency limit for all uploads and downloads. Start with the default of 8."
        }
        " · 有新版本可用" => " · Update available",
        "关闭更新窗口" => "Close update window",
        "关闭弹窗" => "Close dialog",
        "账户名称" => "Account name",
        "Session Token  可选 · 临时凭据" => {
            "Session Token  Optional · Temporary credentials"
        }
        "Endpoint  可选 · S3 兼容服务" => "Endpoint  Optional · S3-compatible services",
        "使用路径风格访问" => "Use path-style addressing",
        "MinIO 等服务推荐开启" => "Recommended for MinIO and similar services",
        "文件夹名称" => "Folder name",
        "存储桶名称" => "Bucket name",
        "目标对象键" => "Destination object key",
        "并发数量" => "Concurrency",
        "支持 1–32 路。设置自动保存；降低上限时，已开始的分片会先完成，后续分片按新上限执行。" => {
            "Supports 1–32 concurrent parts. Settings save automatically. Active parts finish before a lower limit takes effect."
        }
        "① 添加账户：填写 Access Key、Secret Key 和 Region。MinIO 等服务可以配置 Endpoint 和路径风格。" => {
            "① Add an account: enter Access Key, Secret Key, and Region. Configure Endpoint and path-style addressing for services such as MinIO."
        }
        "② 选择存储桶，双击文件夹浏览。选择文件后可下载、复制、分享或删除。" => {
            "② Select a bucket and double-click folders to browse. Select files to download, copy, share, or delete."
        }
        "③ 使用 ⌘ / Ctrl + F 搜索，Escape 关闭弹窗或取消选择。" => {
            "③ Press ⌘ / Ctrl + F to search. Press Escape to close dialogs or clear selection."
        }
        "删除目录会同时删除其中所有文件和子目录。分享链接有效期 1 小时。传输活动支持暂停、继续和失败重试；关闭应用后，再次打开可从断点继续。" => {
            "Deleting a folder deletes all its contents. Share links expire in 1 hour. Transfers support pause, resume, retry, and resuming after an app restart."
        }
        "首次使用需具备相应的 S3 权限；Ubuntu 需启动支持 Secret Service 的密钥库。" => {
            "S3 permissions are required. On Ubuntu, start a keyring that supports Secret Service."
        }
        "跨平台 S3 文件工作空间" => "A cross-platform S3 file workspace",
        "正在更新…" => "Updating…",
        "下载并安装" => "Download and Install",
        "打开下载页" => "Open Downloads",
        "保留任务" => "Keep Transfer",
        "取消" => "Cancel",
        "处理中…" => "Working…",
        "正在检查新版本…" => "Checking for updates…",
        "签名校验通过，正在安装更新…" => "Signature verified. Installing update…",
        "正在下载并校验更新包…" => "Downloading and verifying update…",
        "已是最新版本。" => "You’re up to date.",
        "确认后下载更新包，通过签名校验后安装并重启应用。" => {
            "After confirmation, the update will be downloaded, verified, installed, and the app restarted."
        }
        "当前安装方式需前往发布页下载新版本。账户配置与传输断点会保留。" => {
            "Download the new version from the releases page. Account settings and transfer checkpoints will be preserved."
        }
        "请先暂停或完成所有传输，再安装更新。" => {
            "Pause or finish all transfers before installing an update."
        }
        "服务" => "Services",
        "隐藏 S3 Studio" => "Hide S3 Studio",
        "隐藏其他应用" => "Hide Others",
        "显示全部" => "Show All",
        "退出 S3 Studio" => "Quit S3 Studio",
        "编辑" => "Edit",
        "撤销" => "Undo",
        "重做" => "Redo",
        "剪切" => "Cut",
        "粘贴" => "Paste",
        "全选" => "Select All",
        "语言" => "Language",
        "跟随系统" => "Follow System",
        "选择上传到 S3 的文件" => "Select files to upload to S3",
        "请选择文件下载。" => "Select a file to download.",
        "保存 S3 对象" => "Save S3 Object",
        "远端对象已变化" => "Remote object changed",
        "源文件已变化" => "Source file changed",
        "防止覆盖" => "Prevent overwrite",
        "请选择普通文件。" => "Select a regular file.",
        "文件长度变化或断点文件不完整。" => "File length changed or checkpoint is incomplete.",
        "传输记录版本或并发设置无效。" => "Invalid transfer record version or concurrency setting.",
        "传输任务不存在。" => "Transfer not found.",
        "对象键无效。" => "Invalid object key.",
        "保存路径无效。" => "Invalid save path.",
        "保存文件名无效。" => "Invalid file name.",
        "文件超过 5 TiB 上传上限。" => "File exceeds the 5 TiB upload limit.",
        "正在更新应用，请完成更新后再添加传输任务。" => "An app update is in progress. Finish it before adding transfers.",
        "此文件已有未结束的传输任务，请继续或取消原任务。" => "This file already has an unfinished transfer. Resume or cancel it first.",
        "正在更新应用，请完成更新后再继续传输。" => "An app update is in progress. Finish it before resuming transfers.",
        "任务正在运行或停止中，请稍后重试。" => "Transfer is running or stopping. Try again later.",
        "任务已经结束。" => "Transfer has already ended.",
        "请先暂停或完成正在运行的传输任务，再安装应用更新。" => "Pause or finish all active transfers before installing an app update.",
        "应用更新已经在进行中。" => "An app update is already in progress.",
        "正在提交文件，完成后即可结束。" => "Finalizing the file. Wait for it to finish.",
        "任务当前没有运行。" => "Transfer is not running.",
        "正在提交文件，无法取消。" => "Cannot cancel while finalizing the file.",
        "本地源文件已变化，请取消任务后重新上传。" => "Local source file changed. Cancel this transfer and upload again.",
        "源文件内容已变化，不能继续原上传。" => "Source contents changed. This upload cannot be resumed.",
        "服务端分片编号无效。" => "Invalid part number returned by server.",
        "服务端分片长度与源文件不一致。" => "Server part length differs from the source file.",
        "服务端分片缺少 ETag。" => "Server part is missing an ETag.",
        "服务端未返回分片分页标记。" => "Server did not return a part pagination token.",
        "服务未返回 multipart UploadId。" => "Server did not return a multipart UploadId.",
        "服务未返回对象大小。" => "Server did not return the object size.",
        "对象大小无效。" => "Invalid object size.",
        "服务未返回对象 ETag，无法安全进行断点下载。" => "Server did not return an ETag. Download cannot be resumed safely.",
        "远端对象已变化，请取消任务后重新下载。" => "Remote object changed. Cancel this transfer and download again.",
        "下载临时文件大小变化，请取消后重新下载。" => "Temporary download file size changed. Cancel and download again.",
        "下载断点文件已丢失，请取消后重新下载。" => "Download checkpoint file is missing. Cancel and download again.",
        "下载断点分片编号无效。" => "Invalid download checkpoint part number.",
        "分片任务意外结束。" => "Part task ended unexpectedly.",
        "已完成分片长度不完整。" => "Completed part has an incomplete length.",
        "上传期间源文件已变化，请取消任务后重新上传。" => "Source file changed during upload. Cancel and upload again.",
        "上传期间源文件内容已变化，已阻止提交。" => "Source contents changed during upload. Finalization was blocked.",
        "UploadId 丢失。" => "UploadId is missing.",
        "保存位置的文件已变化，为防止覆盖新文件，已保留下载断点。请取消后重新选择保存位置。" => "The destination file changed. The download checkpoint was kept to avoid overwriting it. Cancel and select another destination.",
        "凭据数据损坏，请重新设置账户凭据。" => "Credential data is corrupted. Set your account credentials again.",
        "凭据不存在，请在账户设置中重新填写访问密钥。" => "Credentials not found. Enter access keys in Account Settings.",
        "账户名称需为 1–80 个字符。" => "Account name must contain 1–80 characters.",
        "请输入有效的 Region，例如 us-east-1。" => "Enter a valid Region, such as us-east-1.",
        "Endpoint 必须是完整的 HTTP / HTTPS URL。" => "Endpoint must be a complete HTTP / HTTPS URL.",
        "Endpoint 只能使用 HTTP / HTTPS，且不能包含凭据、查询参数或片段。" => "Endpoint must use HTTP / HTTPS without credentials, query parameters, or fragments.",
        "更新凭据时必须同时填写 Access Key 和 Secret Key。" => "Enter both Access Key and Secret Key to update credentials.",
        "更新 Session Token 时请同时填写 Access Key 和 Secret Key。" => "Enter both Access Key and Secret Key to update Session Token.",
        "账户不存在，请重新加载。" => "Account not found. Reload and try again.",
        "新账户必须提供访问凭据。" => "New accounts require access credentials.",
        "此账户仍有未结束的传输，请先完成或取消任务再移除账户。" => "This account has unfinished transfers. Finish or cancel them before removing it.",
        "账户不存在。" => "Account not found.",
        "存储桶名称需为 3–63 个小写字母、数字、点或连字符，且符合 S3 命名规则。" => "Bucket names must contain 3–63 lowercase letters, digits, dots, or hyphens and follow S3 naming rules.",
        "文件夹路径无效。" => "Invalid folder path.",
        "请选择有效文件或目录，不能删除存储桶根目录。" => "Select a valid file or folder. The bucket root cannot be deleted.",
        "服务端列出的对象缺少键名。" => "An object listed by the server has no key.",
        "服务端返回了所选目录以外的对象，已停止删除。" => "Server returned an object outside the selected folder. Deletion stopped.",
        "服务端未返回目录分页标记，已停止删除。" => "Server did not return a folder pagination token. Deletion stopped.",
        "服务端目录分页标记重复，已停止删除。" => "Server repeated a folder pagination token. Deletion stopped.",
        "未知错误" => "Unknown error",
        "请输入不同且有效的目标对象键（最多 1024 字节）。" => "Enter a different valid destination key (up to 1024 bytes).",
        "目标对象已存在，请换一个对象键。" => "Destination already exists. Choose another object key.",
        "有效期需为 60 秒到 7 天。" => "Expiration must be between 60 seconds and 7 days.",
        _ => text,
    }
}
#[macro_export]
macro_rules! lf {
("已删除至少 {} 个对象，部分对象删除失败，请刷新列表确认：{}", $($args:tt)*) => { if $crate::i18n::english() { format!("Deleted at least {} objects; some deletions failed. Refresh the list to confirm: {}", $($args)*) } else { format!("已删除至少 {} 个对象，部分对象删除失败，请刷新列表确认：{}", $($args)*) } };
("已完成 {} 个对象的删除，本批请求失败，请刷新列表确认：{}", $($args:tt)*) => { if $crate::i18n::english() { format!("Deleted {} objects; this batch failed. Refresh the list to confirm: {}", $($args)*) } else { format!("已完成 {} 个对象的删除，本批请求失败，请刷新列表确认：{}", $($args)*) } };
("{e}；凭据回滚失败：{rollback}", $($args:tt)*) => { if $crate::i18n::english() { format!("{e}; credential rollback failed: {rollback}", $($args)*) } else { format!("{e}；凭据回滚失败：{rollback}", $($args)*) } };
("{error}；凭据回滚失败：{rollback_error}", $($args:tt)*) => { if $crate::i18n::english() { format!("{error}; credential rollback failed: {rollback_error}", $($args)*) } else { format!("{error}；凭据回滚失败：{rollback_error}", $($args)*) } };
("系统密钥库操作失败：{e}。请确认密钥库已解锁；Ubuntu 需启用 Secret Service。", $($args:tt)*) => { if $crate::i18n::english() { format!("System keychain operation failed: {e}. Unlock your keychain; on Ubuntu, enable Secret Service.", $($args)*) } else { format!("系统密钥库操作失败：{e}。请确认密钥库已解锁；Ubuntu 需启用 Secret Service。", $($args)*) } };
("无法替换账户配置：{e}", $($args:tt)*) => { if $crate::i18n::english() { format!("Unable to replace account settings: {e}", $($args)*) } else { format!("无法替换账户配置：{e}", $($args)*) } };
("无法保存账户配置：{e}", $($args:tt)*) => { if $crate::i18n::english() { format!("Unable to save account settings: {e}", $($args)*) } else { format!("无法保存账户配置：{e}", $($args)*) } };
("{error}；分片清理失败：{}", $($args:tt)*) => { if $crate::i18n::english() { format!("{error}; failed to clean up parts: {}", $($args)*) } else { format!("{error}；分片清理失败：{}", $($args)*) } };
("无法保存传输断点：{e}", $($args:tt)*) => { if $crate::i18n::english() { format!("Unable to save transfer checkpoint: {e}", $($args)*) } else { format!("无法保存传输断点：{e}", $($args)*) } };
("已添加 {count} 个上传任务，部分失败：{}", $($args:tt)*) => { if $crate::i18n::english() { format!("Added {count} uploads; some failed: {}", $($args)*) } else { format!("已添加 {count} 个上传任务，部分失败：{}", $($args)*) } };
    ("暂时无法检查更新：{error}", $($args:tt)*) => { if $crate::i18n::english() { format!("Unable to check for updates: {error}", $($args)*) } else { format!("暂时无法检查更新：{error}", $($args)*) } };
    ("更新未完成，当前版本保持不变：{error}", $($args:tt)*) => { if $crate::i18n::english() { format!("Update incomplete; the current version is unchanged: {error}", $($args)*) } else { format!("更新未完成，当前版本保持不变：{error}", $($args)*) } };
    ("未知操作：{command}", $($args:tt)*) => { if $crate::i18n::english() { format!("Unknown operation: {command}", $($args)*) } else { format!("未知操作：{command}", $($args)*) } };
    ("已删除 {} 个对象", $($args:tt)*) => { if $crate::i18n::english() { format!("Deleted {} objects", $($args)*) } else { format!("已删除 {} 个对象", $($args)*) } };
    ("已添加 {count} 个上传任务，可在传输活动中暂停或继续", $($args:tt)*) => { if $crate::i18n::english() { format!("Added {count} uploads; pause or resume them in Transfers", $($args)*) } else { format!("已添加 {count} 个上传任务，可在传输活动中暂停或继续", $($args)*) } };
    ("无法启动 S3 Studio：{error}", $($args:tt)*) => { if $crate::i18n::english() { format!("Unable to start S3 Studio: {error}", $($args)*) } else { format!("无法启动 S3 Studio：{error}", $($args)*) } };
    ("{active} 项正在传输", $($args:tt)*) => { if $crate::i18n::english() { format!("{active} active", $($args)*) } else { format!("{active} 项正在传输", $($args)*) } };
    ("{paused} 项已暂停", $($args:tt)*) => { if $crate::i18n::english() { format!("{paused} paused", $($args)*) } else { format!("{paused} 项已暂停", $($args)*) } };
    ("{} 路并发", $($args:tt)*) => { if $crate::i18n::english() { format!("{} concurrent", $($args)*) } else { format!("{} 路并发", $($args)*) } };
    ("暂停 {}", $($args:tt)*) => { if $crate::i18n::english() { format!("Pause {}", $($args)*) } else { format!("暂停 {}", $($args)*) } };
    ("取消 {}", $($args:tt)*) => { if $crate::i18n::english() { format!("Cancel {}", $($args)*) } else { format!("取消 {}", $($args)*) } };
    ("{} 个文件{}", $($args:tt)*) => { if $crate::i18n::english() { format!("{} files{}", $($args)*) } else { format!("{} 个文件{}", $($args)*) } };
    ("已选 {} 项", $($args:tt)*) => { if $crate::i18n::english() { format!("{} selected", $($args)*) } else { format!("已选 {} 项", $($args)*) } };
    ("{} 个项目{}{}", $($args:tt)*) => { if $crate::i18n::english() { format!("{} items{}{}", $($args)*) } else { format!("{} 个项目{}{}", $($args)*) } };
    (" · 已选择 {} 项", $($args:tt)*) => { if $crate::i18n::english() { format!(" · {} selected", $($args)*) } else { format!(" · 已选择 {} 项", $($args)*) } };
    ("{} · 当前已加载", $($args:tt)*) => { if $crate::i18n::english() { format!("{} · Loaded so far", $($args)*) } else { format!("{} · 当前已加载", $($args)*) } };
    ("选择 {}", $($args:tt)*) => { if $crate::i18n::english() { format!("Select {}", $($args)*) } else { format!("选择 {}", $($args)*) } };
    ("查看 {}", $($args:tt)*) => { if $crate::i18n::english() { format!("View {}", $($args)*) } else { format!("查看 {}", $($args)*) } };
    ("将递归删除 {folders} 个目录及其中所有文件和子目录，并删除 {} 个所选文件。未启用版本控制的对象无法恢复。", $($args:tt)*) => { if $crate::i18n::english() { format!("Recursively delete {folders} folders and all their contents, plus {} selected files. Objects without versioning cannot be recovered.", $($args)*) } else { format!("将递归删除 {folders} 个目录及其中所有文件和子目录，并删除 {} 个所选文件。未启用版本控制的对象无法恢复。", $($args)*) } };
    ("即将删除 {} 个文件。未启用版本控制的对象无法恢复。", $($args:tt)*) => { if $crate::i18n::english() { format!("Delete {} files. Objects without versioning cannot be recovered.", $($args)*) } else { format!("即将删除 {} 个文件。未启用版本控制的对象无法恢复。", $($args)*) } };
    ("{} 的断点将被清理；已经存在的源文件和目标文件不受影响。", $($args:tt)*) => { if $crate::i18n::english() { format!("The checkpoint for {} will be removed. Existing source and destination files are unaffected.", $($args)*) } else { format!("{} 的断点将被清理；已经存在的源文件和目标文件不受影响。", $($args)*) } };
    ("当前版本 {}{}", $($args:tt)*) => { if $crate::i18n::english() { format!("Current version {}{}", $($args)*) } else { format!("当前版本 {}{}", $($args)*) } };
    ("版本 {}", $($args:tt)*) => { if $crate::i18n::english() { format!("Version {}", $($args)*) } else { format!("版本 {}", $($args)*) } };
    ("上次检查：{}", $($args:tt)*) => { if $crate::i18n::english() { format!("Last checked: {}", $($args)*) } else { format!("上次检查：{}", $($args)*) } };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    System,
    Chinese,
    English,
}
static PREFERENCE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
impl Language {
    fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Chinese => "zh-CN",
            Self::English => "en",
        }
    }
    fn parse(code: &str) -> Self {
        match code.trim() {
            "zh-CN" => Self::Chinese,
            "en" => Self::English,
            _ => Self::System,
        }
    }
}
pub fn preference() -> Language {
    match PREFERENCE.load(Ordering::Relaxed) {
        1 => Language::Chinese,
        2 => Language::English,
        _ => Language::System,
    }
}
pub fn resolve(language: Language, locale: &str) -> bool {
    match language {
        Language::Chinese => false,
        Language::English => true,
        Language::System => !locale.to_ascii_lowercase().starts_with("zh"),
    }
}
fn apply(language: Language) {
    PREFERENCE.store(language as u8, Ordering::Relaxed);
    ENGLISH.store(
        resolve(
            language,
            &sys_locale::get_locale().unwrap_or_else(|| "en".into()),
        ),
        Ordering::Relaxed,
    );
}
fn settings_path() -> std::io::Result<std::path::PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("com.s3studio.desktop").join("language"))
        .ok_or_else(|| std::io::Error::other("Unable to locate settings directory"))
}
pub fn initialize() {
    let saved = settings_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    apply(Language::parse(&saved));
}
pub fn select(language: Language) -> std::io::Result<()> {
    let path = settings_path()?;
    save_preference(&path, language)?;
    apply(language);
    Ok(())
}

pub fn translate_placeholder(text: &str) -> String {
    let source = match text {
        "For example: Production / Personal" => "例如：生产环境 / 个人存储",
        "Leave blank to keep existing credentials" => "留空保留现有凭据",
        "Your Secret Access Key" => "你的 Secret Access Key",
        "For temporary credentials; leave blank when updating to clear" => {
            "使用临时凭据时填写；更新凭据时留空可清除"
        }
        _ => text,
    };
    t(source).to_owned()
}

fn save_preference(path: &std::path::Path, language: Language) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, language.code())?;
    std::fs::rename(temporary, path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_system_language_and_honors_overrides() {
        for locale in ["zh-CN", "zh_TW.UTF-8", "ZH-Hans", "zh-HK"] {
            assert!(!resolve(Language::System, locale));
        }
        for locale in ["en-US", "fr-FR", "ja-JP", ""] {
            assert!(resolve(Language::System, locale));
        }
        assert!(resolve(Language::English, "zh-CN"));
        assert!(!resolve(Language::Chinese, "en-US"));
    }
    #[test]
    fn translates_ui_and_preserves_unknown_content() {
        assert_eq!(translate("关于 S3 Studio", true), "About S3 Studio");
        assert_eq!(translate("正在检查新版本…", true), "Checking for updates…");
        assert_eq!(translate("账户设置", true), "Account Settings");
        assert_eq!(translate("账户不存在。", true), "Account not found.");
        assert_eq!(translate("账户设置", false), "账户设置");
        assert_eq!(translate("我的文件.txt", true), "我的文件.txt");
    }
    // Run in isolation because the production locale is shared across UI and worker threads.
    #[test]
    #[ignore = "Run separately: mutates the process-wide language"]
    fn english_runtime_smoke() {
        apply(Language::English);
        assert_eq!(t("关于 S3 Studio"), "About S3 Studio");
        let count = 3;
        assert_eq!(crate::lf!("已删除 {} 个对象", count), "Deleted 3 objects");
        assert_eq!(crate::lf!("选择 {}", "我的文件.txt"), "Select 我的文件.txt");
        assert_eq!(
            translate_placeholder("例如：生产环境 / 个人存储"),
            "For example: Production / Personal"
        );
        apply(Language::Chinese);
        assert_eq!(t("关于 S3 Studio"), "关于 S3 Studio");
        assert_eq!(
            translate_placeholder("For example: Production / Personal"),
            "例如：生产环境 / 个人存储"
        );
    }
    #[test]
    fn language_preference_round_trips() {
        let dir = std::env::temp_dir().join(format!("s3-language-{}", uuid::Uuid::new_v4()));
        let path = dir.join("language");
        for language in [Language::English, Language::Chinese, Language::System] {
            save_preference(&path, language).unwrap();
            assert_eq!(
                Language::parse(&std::fs::read_to_string(&path).unwrap()),
                language
            );
        }
        assert_eq!(Language::parse("unknown"), Language::System);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
