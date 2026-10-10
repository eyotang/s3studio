import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import {
  copyFile,
  mkdir,
  readFile,
  readdir,
  writeFile,
} from "node:fs/promises";
import path from "node:path";

const [input, output] = process.argv.slice(2);
if (!input || !output)
  throw new Error("Usage: prepare-release.mjs INPUT OUTPUT");
const pkg = JSON.parse(await readFile("package.json", "utf8"));
const lock = JSON.parse(await readFile("package-lock.json", "utf8"));
const tauri = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
const cargo = await readFile("src-tauri/Cargo.toml", "utf8");
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const version = pkg.version;
if (
  [lock.version, lock.packages[""].version, tauri.version, cargoVersion].some(
    (value) => value !== version,
  ) ||
  process.env.GITHUB_REF_NAME !== `v${version}`
)
  throw new Error("Release tag and all application versions must match");

async function walk(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await walk(file)));
    else if (entry.isFile()) files.push(file);
  }
  return files;
}

const files = await walk(input);
const packages = [
  [".exe", "windows_x64-setup.exe"],
  [".msi", "windows_x64.msi"],
  [".dmg", "macOS_universal.dmg"],
  [".deb", "ubuntu22.04_amd64.deb"],
  [".AppImage", "linux_x86_64.AppImage"],
];
await mkdir(output, { recursive: true });
if ((await readdir(output)).length) throw new Error("Output must be empty");
const checksums = [];
for (const [extension, suffix] of packages) {
  const matches = files.filter((file) => file.endsWith(extension));
  if (matches.length !== 1)
    throw new Error(
      `Expected exactly one ${extension} package, got ${matches.length}`,
    );
  const source = matches[0];
  if (!path.basename(source).includes(version))
    throw new Error(`Package version mismatch: ${source}`);
  const name = `S3-Studio_${version}_${suffix}`;
  const target = path.join(output, name);
  await copyFile(source, target);
  const hash = createHash("sha256");
  let size = 0;
  for await (const chunk of createReadStream(target)) {
    hash.update(chunk);
    size += chunk.length;
  }
  if (!size) throw new Error(`Empty package: ${name}`);
  checksums.push(`${hash.digest("hex")}  ${name}`);
  console.log(`${name}: ${size} bytes`);
}
await writeFile(
  path.join(output, "SHA256SUMS.txt"),
  checksums.join("\n") + "\n",
);
