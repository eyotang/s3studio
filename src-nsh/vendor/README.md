# Updater compatibility patch

The vendored `tauri-plugin-updater` 2.13.2 adds `UpdaterBuilder::standalone` so the Native Shell app can pass the installed version and updater configuration. It uses the upstream installer selection, download, signature verification, and installation code.

The desktop app reads the updater settings from `src-nsh/tauri.conf.json`. Tauri's bundler writes the installer type into the executable. The Windows and AppImage installers use the same signing key and manifest as previous releases. macOS continues to open the release page for manual updates.
