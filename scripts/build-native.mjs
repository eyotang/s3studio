import { spawnSync } from "node:child_process";
import { copyFile, mkdir, readFile } from "node:fs/promises";
import path from "node:path";

// Keep the existing Tauri installer/signing pipeline; supply the GPUI executable.
const universal = process.argv.includes("--universal");
const targets = universal
  ? ["aarch64-apple-darwin", "x86_64-apple-darwin"]
  : [undefined];
const pkg = JSON.parse(await readFile("package.json", "utf8"));
for (const file of ["src-gpui/Cargo.toml", "src-core/Cargo.toml"]) {
  const cargo = await readFile(file, "utf8");
  if (cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1] !== pkg.version)
    throw new Error(`Version mismatch in ${file}`);
}
function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status || 1);
}
for (const target of targets) {
  run("cargo", [
    "build",
    "--release",
    "--locked",
    "--manifest-path",
    "src-gpui/Cargo.toml",
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
    ...targets.map((target) => `src-gpui/target/${target}/release/${binary}`),
    "-output",
    path.join(destination, binary),
  ]);
} else {
  await copyFile(
    path.join("src-gpui/target/release", binary),
    path.join(destination, binary),
  );
}
console.log(`GPUI executable ready: ${path.join(destination, binary)}`);
