# Language catalogs

`en.json`, `ko.json`, `ja.json`, `fr.json`, `de.json`, and `ru.json` use the original Chinese message as the key. Chinese uses the original message directly. Every catalog must contain the same keys, preserving all `{}` and named placeholders such as `{count}`.

After editing a catalog, run from the repository root:

```bash
node scripts/sync-i18n.mjs
cargo fmt --manifest-path src-gpui/Cargo.toml
cargo test --locked --manifest-path src-core/Cargo.toml
cargo test --locked --manifest-path src-core/Cargo.toml multilingual_runtime_smoke -- --ignored --test-threads=1
cargo test --locked --manifest-path src-gpui/Cargo.toml
```

The synchronization script validates catalog keys and placeholders and updates the compile-time format macros in `src-core/src/i18n.rs`. `src-core/build.rs` embeds the static translations at build time. Keep the generated macros in sync in the same commit as catalog edits. Run the runtime smoke test separately because the production language is shared by UI and worker threads.

The language list, native names, persisted language codes, and system-locale matching live in `src-core/src/i18n.rs`. The settings panel consumes this list. Preserve account names, object keys, file names, secrets, and raw service errors as user or server content; translate only application-owned messages.
