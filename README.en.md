# S3 Studio

[简体中文](README.md) · [English](README.en.md) · [한국어](README.ko.md) · [日本語](README.ja.md) · [Français](README.fr.md) · [Deutsch](README.de.md) · [Русский](README.ru.md)

S3 Studio is a cross-platform desktop S3 client for managing cloud files in one clear workspace. Connect AWS S3, MinIO, or other S3-compatible services, manage multiple accounts, browse buckets, and upload or download files. Accounts are unlimited and credentials stay in your system keychain.

![S3 Studio](artifacts/workspace-preview.png)

## Product features

- Multiple accounts with custom Endpoint, Region, path-style addressing, and temporary credentials.
- Folder navigation, breadcrumbs, search, sorting, list/grid views, selection, and object details.
- Upload, download, create folders and buckets, copy within a bucket, delete multiple objects, and share signed download links.
- Transfer progress, speed, pause, resume, retry, and checkpoint recovery after an app restart.
- A shared concurrency limit across accounts: 8 by default, configurable from 1 to 32.
- System, light, and dark themes; Chinese, English, Korean, Japanese, French, German, and Russian.
- Credentials stored in macOS Keychain, Windows Credential Manager, or Linux Secret Service.

## Download and install

Download the latest version from [GitHub Releases](https://github.com/eyotang/s3studio/releases/latest). Verify downloads against the release’s `SHA256SUMS.txt` before installing.

| Platform                    | Package          | Installation                                        |
| --------------------------- | ---------------- | --------------------------------------------------- |
| macOS Apple Silicon / Intel | Universal `.dmg` | Drag S3 Studio into Applications                    |
| Windows x64                 | `.exe` / `.msi`  | Run the installer                                   |
| Ubuntu 22.04+ amd64         | `.deb`           | Open with the system package installer              |
| Linux x86_64                | `.AppImage`      | Make executable, then run                           |
| Omarchy / Arch Linux x86_64 | `.pkg.tar.zst`   | Install using `sudo pacman -U` and the package path |

### If macOS cannot open the app

Installers are not currently system-code-signed or notarized by Apple. Move the app to Applications, verify its official release source and SHA-256, then run:

```bash
sudo xattr -d com.apple.quarantine "/Applications/S3 Studio.app"
```

On Linux, run and unlock a Secret Service keyring such as GNOME Keyring, and provide your desktop’s file chooser portal. Allow AppImage execution in file properties. Omarchy packages are not in AUR; download new packages to upgrade with `pacman -U`. Remove the app with `sudo pacman -R s3-studio`; personal settings are retained.

## Getting started

### Connect an account

Choose Add Account and enter a name, Access Key ID, Secret Access Key, and Region. AWS usually needs no Endpoint; bucket regions are detected automatically. Temporary credentials also need a Session Token. For MinIO, use an Endpoint such as `http://localhost:9000`, Region `us-east-1`, and path-style addressing. Use HTTPS in production.

Your account needs the S3 permissions required by each operation. Double-click folders to browse and files to show details. Select objects to download, copy, share, or delete. Folder deletion recursively deletes its contents after confirmation. Uploads overwrite matching object names; share links are valid for 1 hour and may expire earlier with temporary credentials.

### Language, theme, and transfers

Open Settings from the sidebar or, on macOS, S3 Studio → Settings… (⌘,). Language and theme changes take effect immediately and are saved locally. Follow System detects supported system languages and falls back to English. Set concurrency to 1–32 and choose Save Settings. The native macOS titlebar follows the chosen theme.

Use Transfers to pause, resume, retry, or cancel. Completed parts are retained when paused. After restarting, unfinished transfers remain paused until you resume them. Cancellation cleans up checkpoints and temporary content. Changes to the source, remote object, or destination can block unsafe resumption or overwrites.

### Updates and limits

Check for updates from the macOS app menu or the version label. Release builds check after 20 seconds and every 24 hours. Windows NSIS/MSI and Linux AppImage support verified in-app installation; finish or pause transfers first. macOS and deb installations open the release page for manual installation. Accounts and checkpoints are retained.

Uploads are limited to 5 TiB per file; server-side single-object copying uses S3 CopyObject’s 5 GB limit. Deletion does not remove version history. SSO/AssumeRole, ACL/policy editing, version management, cross-account copying, directory sync, Glacier restore, folder downloads, and CloudFront management are not included. S3-compatible service behavior depends on its implementation.

## Development and contribution

Built with GPUI, GPUI Kit, Rust, and AWS SDK for Rust. `src-gpui` is the native UI, `src-core` the shared backend, and `src`/`src-tauri` retain the original React/Tauri reference. Install Node.js 22.12+ and stable Rust. macOS needs Xcode Command Line Tools; Windows needs MSVC C++ Build Tools and WebView2. Linux needs the native libraries listed in the [development reference](README.md#开发与贡献).

```bash
npm ci
npm run dev:native
```

For a desktop preview using sample data, run `npm run preview:native`. `npm run dev` starts the browser reference at `http://127.0.0.1:1420`; it does not connect to cloud storage or save credentials.

Run the checks relevant to your changes:

```bash
npm test
npm run test:ui
npm run build
cargo test --locked --manifest-path src-core/Cargo.toml
cargo test --locked --manifest-path src-gpui/Cargo.toml
```

Build the native executable with `npm run build:native` and package it with `npm run tauri bundle -- --ci`. Platform packages are built on their target operating systems. See the [development reference](README.md#开发与贡献) for Linux dependencies, Universal builds, storage paths, and release signing. Update language catalogs in `src-core/locales`, then run `node scripts/sync-i18n.mjs` and format Rust before testing.

Issues should include the app version, operating system, reproduction steps, and errors. Remove credentials and private data from logs and screenshots. Pull requests should explain the problem, resulting behavior, and validation; include screenshots for UI changes. Tests use a local S3 simulator rather than real cloud accounts. Follow [AGENTS.md](AGENTS.md) when publishing: English release notes first, Chinese second, with the macOS xattr instructions in both.

This project is independently implemented and is not an official AWS product. See the [Chinese technical reference](README.md), [release notes](docs/releases), and [brand assets](assets/brand).
