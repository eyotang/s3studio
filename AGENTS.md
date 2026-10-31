# 发布规则

每次创建或更新版本发布时，必须在 `docs/releases/<版本>.md` 和 GitHub Release 描述中包含 macOS 无法打开应用时的处理说明及以下完整命令；即使本次更新仅涉及其他平台，也要保留该说明：

## macOS 无法打开应用

将 S3 Studio 放入「应用程序」文件夹，核对官方 Release 来源及 SHA-256 后，在终端执行：

```bash
sudo xattr -d com.apple.quarantine "/Applications/S3 Studio.app"
```

发布完成后，读取 GitHub Release 描述，确认该说明和命令已实际显示。复用已有发布说明时也必须检查并补齐；无需用户再次提醒。
