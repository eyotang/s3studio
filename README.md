# S3 Studio

基于 **Tauri 2 + React + TypeScript + Rust + AWS SDK for Rust** 的跨平台图形化 S3 客户端。以 Apple 的清晰层级、柔和材质、留白和原生交互为设计参考，支持 macOS、Ubuntu 和 Windows，**账户数量不设上限**。

![工作空间预览](artifacts/workspace-preview.png)

## 已实现

- 添加、编辑、切换和移除任意数量的账户。
- AWS S3 及自定义 Endpoint 的 S3 兼容服务；支持路径风格、Region 和临时 Session Token。
- 凭据通过 macOS Keychain、Windows Credential Manager、Ubuntu Secret Service 保存；前端不接收已保存的凭据，配置文件不存储访问密钥。
- 存储桶列表、创建存储桶、AWS 桶区域自动探测。
- 文件夹浏览、面包屑导航、新建文件夹、每页 1,000 项分页加载。
- 搜索当前已加载对象、按名称/大小/修改时间排序、列表与网格切换、多选。
- 并发分片上传：按文件偏移流式读取，不将整个文件放入内存；SDK 负责可重试请求的重试。
- 并发 Range 下载：各分片写入临时文件的不同区间，全部完成后才替换目标文件。
- 批量删除文件、同桶复制对象、生成 1 小时预签名下载链接。
- 对象大小、修改时间、Content-Type、ETag 和自定义元数据查看。
- 传输活动：进度、速度、暂停、继续、失败重试、取消及断点清理；应用重启后可手动继续。
- 应用级共享并发上限默认 **8 路**，支持 **1–32 路**，运行中可修改并自动保存。
- ⌘ / Ctrl + F 搜索、Escape 关闭弹窗/清除选择、弹窗焦点循环。
- Windows / macOS / Ubuntu 的 GitHub Actions 构建配置。

参考 [S3 Browser 的公开功能](https://s3browser.com/)，本项目采用独立实现，并非 AWS 官方产品。Tauri 配置和 IPC 依据 [Tauri 官方文档](https://v2.tauri.app/)，S3 请求由 [AWS 官方 Rust SDK](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/welcome.html) 完成。

Logo 以 Apple Design 的克制、留白和几何层级为参考：保留浅蓝色圆角底，采用三层等距几何线条表达对象存储。侧栏、桌面与安装包使用同一标志。最终 SVG 源文件和设计说明位于 `assets/brand/`，应用内图标与 favicon 位于 `public/`，桌面 PNG / ICO / ICNS 位于 `src-tauri/icons/`。重新导出使用 `npm run tauri -- icon assets/brand/s3-studio-logo.svg`。

## 开发环境

- Node.js 22.12+（本机验证：22.23.1）。
- 最新稳定版 Rust（本机验证：1.97.0；依赖版本固定在 `src-tauri/Cargo.lock`）。
- Windows：MSVC C++ Build Tools 和 WebView2。
- macOS：Xcode Command Line Tools。
- Ubuntu：以下原生开发依赖；桌面会话中需要运行且解锁支持 Secret Service 的密钥库，如 GNOME Keyring。

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev libssl-dev librsvg2-dev libayatana-appindicator3-dev libdbus-1-dev gnome-keyring
```

安装与启动：

```bash
npm ci
npm run tauri dev
```

只查看界面：

```bash
npm run dev
```

打开 `http://127.0.0.1:1420`。浏览器模式展示明确标注的示例账户和文件，**不会连接云存储，也不会保存凭据或执行真实操作**。桌面模式不注入示例账户。

## 连接账户

点击侧栏「添加账户」，输入名称、Access Key ID、Secret Access Key 和 Region。

AWS 一般不填 Endpoint；访问桶时自动探测实际区域。临时凭据需同时输入 Session Token；更新 Access Key / Secret Key 时可以留空 Token 来清除原有 Token。编辑账户时两个密钥同时留空，会保留完整原有凭据。

MinIO 例子：Endpoint 为 `http://localhost:9000`，Region 为 `us-east-1`，勾选「使用路径风格访问」。正式环境建议使用 HTTPS。

账户需要相应的 S3 权限：列出桶需要 `s3:ListAllMyBuckets`，浏览需要 `s3:ListBucket`，文件操作需要 `s3:GetObject`、`s3:PutObject`、`s3:DeleteObject`；恢复分片上传需要 `s3:ListMultipartUploadParts`，取消清理需要 `s3:AbortMultipartUpload`，建桶需要 `s3:CreateBucket`。下载版本控制桶的固定对象版本可能需要 `s3:GetObjectVersion`。复制需要源对象读取和目标写入权限；为防止覆盖，复制前会检查目标对象是否存在。权限不足会在界面显示服务错误。

选择文件后，搜索栏右侧提供下载、复制、分享和删除操作。双击文件夹进入，双击文件显示对象详情。

账户元数据文件位于 Tauri 的应用数据目录，文件名 `accounts.json`；凭据位于系统密钥库，服务名 `com.s3studio.desktop`，每个账户使用独立 UUID。

## 并发、暂停与断点续传

点击底部「传输活动」右侧的齿轮进入「传输设置」。默认 8 路并发，所有账户的上传与下载任务共同使用这个上限，而非每个任务各占 8 路。可设为 1–32 的整数；提高上限会增加后续调度，降低上限时已开始的分片先完成，再按新的上限开始分片。文件不足一个分片时只能占用一路。

分片默认 16 MiB，超大文件会增大分片以保持不超过 10,000 片。非空文件使用 Multipart Upload，下载使用带 ETag 条件的 Range 请求；每路缓冲区约 256 KiB，不按完整分片大小分配内存。上传进度与速度按分片完成更新，下载进度按读取字节更新。

「暂停」会停止当前未完成的分片，保留已经成功的分片；「继续」和「重试」会校验断点并补传缺少的分片。未完成分片重新传输，因此并非精确到每个字节续传。关闭应用后，下次启动将未结束任务显示为「已暂停」，点击继续即可恢复，不自动开始网络传输。

配置和传输记录保存在应用数据目录的 `transfers.json`，包含并发上限、文件路径、UploadId、ETag、已完成分片信息和校验摘要，不包含 AK / SK / Token。下载内容保存在目标目录旁的 `.s3studio-<任务 UUID>.part` 文件。取消上传会请求 AbortMultipartUpload，取消下载会删除临时文件；暂停和失败保留断点，清除完成记录不会移除暂停或失败任务。存在未结束任务时不能移除其账户。

上传恢复时核对服务端 ListParts 和本地文件 SHA-256；下载恢复时校验已完成本地分片的 SHA-256，并固定 VersionId 或使用 ETag 检测对象变化。源文件或远端对象变化会阻止续传，保存位置被其他程序修改会阻止覆盖。请取消旧任务后重新发起。服务端生命周期规则若清理了未完成上传，原 UploadId 不能继续使用。

应用数据目录：Windows 为 `%APPDATA%/com.s3studio.desktop`，macOS 为 `~/Library/Application Support/com.s3studio.desktop`，Ubuntu 为 `$XDG_DATA_HOME/com.s3studio.desktop`（默认 `~/.local/share/com.s3studio.desktop`）。

## 检查与构建

```bash
npm test
npm run test:ui
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

界面测试默认用已安装的 Microsoft Edge。若没有 Edge，可将 `playwright.config.ts` 的 `channel` 改为 `chrome`，或删除 `channel` 并运行 `npx playwright install chromium`。

安装包输出到 `src-tauri/target/release/bundle/`：Windows 为 NSIS/MSI，Ubuntu 为 deb/AppImage，macOS 为 app/DMG。应分别在对应操作系统构建；`.github/workflows/build.yml` 已配置三平台构建和产物上传，其中 macOS 生成同时支持 Apple Silicon 和 Intel 的 Universal 包，位于 `src-tauri/target/universal-apple-darwin/release/bundle/`。公开发布前请设置自己的代码签名和 macOS 公证信息，当前配置生成未签名测试包。

Windows Release 构建后可运行 `powershell -NoProfile -File scripts/smoke-desktop.ps1` 检查原生程序启动；测试会结束自己启动的进程，报告保存到 `artifacts/native-startup.json`。

## 当前范围

单文件上传上限为 5 TiB，单次服务端复制受 S3 CopyObject 的 5 GB 限制。大文件上传时请勿修改源文件；实际传输速度取决于网络、磁盘和服务端限制，并发并不保证按路数线性提升。

删除仅作用于所选文件，不递归删除文件夹；删除前有明确确认。上传会覆盖当前目录中的同名对象；版本控制桶的恢复行为由服务端设置决定。分享链接有效期也受临时凭据到期时间影响。

首版暂不包含 SSO/AssumeRole、ACL/策略编辑、版本管理、跨账户复制、目录同步、Glacier 恢复、文件夹批量下载以及 CloudFront 管理。S3 兼容服务的具体 API 兼容程度需以实际联调为准。

本机已通过 **4 项前端基础测试、5 项界面测试、19 项 Rust 测试**，以及 Windows Release 编译和原生启动检查。覆盖并发配置保存与动态调整、本地模拟 S3 的签名、分页、共享并发限制、上传/下载暂停恢复、损坏断点校验、文件变化保护、提交后重启恢复和取消清理。Windows NSIS 安装包位于 `src-tauri/target/release/bundle/nsis/S3 Studio_0.2.2_x64-setup.exe`；校验信息保存于 `artifacts/windows-build.json`，启动报告保存于 `artifacts/native-startup.json`。

未使用真实云账户访问你的数据；macOS、Ubuntu 的原生运行需由三平台构建或各自机器验证。
