import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { appendFile, mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";

const version = "2.12.1";
const builds = {
  "linux-x64": [
    "x86_64-unknown-linux-gnu.tgz",
    "04ef3a9a2ed7b7dc479bef08ab40deee80065c99fc778f79ebce775eb72bbeff",
  ],
  "darwin-arm64": [
    "aarch64-apple-darwin.zip",
    "ee35ee364a0c626df7852d9a5bc0c5394b6440772e9864cfb9b11dcb8d33f527",
  ],
  "darwin-x64": [
    "x86_64-apple-darwin.zip",
    "e5386ba6dadc3fbbdcf46c1f95b700f28caf03db979de50a29adb872022b4a57",
  ],
  "win32-x64": [
    "x86_64-pc-windows-msvc.zip",
    "d3919ebe0bf7013dd98293862784dd37b8f76db2d650e13a3ec11c5dd799c519",
  ],
};
const platform = `${process.platform}-${process.arch}`;
const build = builds[platform];
if (!build) throw new Error(`No bundled Tauri CLI download for ${platform}`);

const [target, expectedHash] = build;
const asset = `cargo-tauri-${target}`;
const directory = await mkdtemp(
  path.join(process.env.RUNNER_TEMP || tmpdir(), "s3-studio-tauri-cli-"),
);
function run(command, args) {
  const result = spawnSync(command, args, { cwd: directory, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} exited ${result.status}`);
}
run("gh", [
  "release",
  "download",
  `tauri-cli-v${version}`,
  "--repo",
  "tauri-apps/tauri",
  "--pattern",
  asset,
]);
const hash = createHash("sha256")
  .update(await readFile(path.join(directory, asset)))
  .digest("hex");
if (hash !== expectedHash) throw new Error(`Checksum mismatch for ${asset}`);
const windows = process.platform === "win32";
const tar = windows
  ? path.join(process.env.SystemRoot, "System32", "tar.exe")
  : "tar";
run(tar, ["-xf", asset]);
run(path.join(directory, windows ? "cargo-tauri.exe" : "cargo-tauri"), [
  "--version",
]);
if (process.env.GITHUB_PATH)
  await appendFile(process.env.GITHUB_PATH, `${directory}\n`);
else console.log(directory);
