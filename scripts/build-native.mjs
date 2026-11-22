import { spawnSync } from "node:child_process";
import { copyFile, mkdir, readFile } from "node:fs/promises";
import path from "node:path";

// Keep the existing Tauri installer/signing pipeline; supply the Native Shell executable.
const universal = process.argv.includes("--universal");
const targets = universal
  ? ["aarch64-apple-darwin", "x86_64-apple-darwin"]
  : [undefined];
const pkg = JSON.parse(await readFile("package.json", "utf8"));
const config = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
for (const file of ["src-nsh/Cargo.toml", "src-core/Cargo.toml"]) {
  const cargo = await readFile(file, "utf8");
  if (cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1] !== pkg.version)
    throw new Error(`Version mismatch in ${file}`);
}
if (config.version !== pkg.version)
  throw new Error("Installer version mismatch");
const env = {
  ...process.env,
  ...(process.platform === "darwin"
    ? { MACOSX_DEPLOYMENT_TARGET: config.bundle.macOS.minimumSystemVersion }
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
  "src-tauri/target",
  ...(universal ? ["universal-apple-darwin"] : []),
  "release",
);
await mkdir(destination, { recursive: true });
if (universal) {
  run("lipo", [
    "-create",
    ...targets.map((target) => `src-nsh/target/${target}/release/${binary}`),
    "-output",
    path.join(destination, binary),
  ]);
} else {
  await copyFile(
    path.join("src-nsh/target/release", binary),
    path.join(destination, binary),
  );
}
console.log(`Native Shell executable ready: ${path.join(destination, binary)}`);
