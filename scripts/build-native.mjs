import { spawnSync } from "node:child_process";
import { copyFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { appConfig, checkApplicationVersions } from "./app-config.mjs";

const universal = process.argv.includes("--universal");
const targets = universal
  ? ["aarch64-apple-darwin", "x86_64-apple-darwin"]
  : [undefined];
await checkApplicationVersions();
const targetDirectory = path.resolve(
  process.env.CARGO_TARGET_DIR || "src-nsh/target",
);
const env = {
  ...process.env,
  CARGO_TARGET_DIR: targetDirectory,
  ...(process.platform === "darwin"
    ? { MACOSX_DEPLOYMENT_TARGET: appConfig.minimumMacOS }
    : {}),
};
function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit", env });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status || 1);
}
for (const target of targets) {
  run("cargo", [
    "build",
    "--release",
    "--locked",
    "--manifest-path",
    "src-nsh/Cargo.toml",
    ...(target ? ["--target", target] : []),
  ]);
}
const binary = process.platform === "win32" ? "s3-studio.exe" : "s3-studio";
const destination = path.join(
  "src-nsh/target/installer",
  ...(universal ? ["universal-apple-darwin"] : []),
  "release",
);
await mkdir(destination, { recursive: true });
if (universal) {
  run("lipo", [
    "-create",
    ...targets.map((target) =>
      path.join(targetDirectory, target, "release", binary),
    ),
    "-output",
    path.join(destination, binary),
  ]);
} else {
  await copyFile(
    path.join(targetDirectory, "release", binary),
    path.join(destination, binary),
  );
}
console.log(`Native Shell executable ready: ${path.join(destination, binary)}`);
