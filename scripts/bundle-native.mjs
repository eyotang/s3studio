import { spawnSync } from "node:child_process";
import path from "node:path";

const result = spawnSync(
  "cargo",
  ["tauri", "bundle", "--ci", ...process.argv.slice(2)],
  {
    cwd: "src-nsh",
    env: {
      ...process.env,
      CARGO_TARGET_DIR: path.resolve("src-nsh/target/installer"),
    },
    stdio: "inherit",
  },
);
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
