# Upstream compatibility patches

- `gpui-pre-macos` 0.3.10: replaces three unstable `Option::map_or_default` calls with equivalent stable `map_or(Default::default(), ...)` calls.
- `tauri-plugin-updater` 2.13.2: adds `UpdaterBuilder::standalone` to supply the existing configuration and application version from GPUI. All upstream download, signature/version verification, installer selection and installation code is unchanged. The Windows app name remains `S3 Studio`; macOS keeps manual release-page updates and never uses the plugin's macOS installer callback.

Both crates retain their upstream licenses. The original Tauri app continues to use the registry updater; this patch is scoped to the GPUI manifest. The native adapter reads updater settings directly from `src-tauri/tauri.conf.json`, including `requireSignedVersion`, the public key, update URL and passive Windows installation mode. The Tauri bundler remains responsible for installer metadata and bundle-type binary patching.
