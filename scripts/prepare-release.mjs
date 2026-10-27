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
import { spawnSync } from "node:child_process";
import { verifyUpdateSignature } from "./update-signature.mjs";

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
  [".pkg.tar.zst", "omarchy_x86_64.pkg.tar.zst"],
];
await mkdir(output, { recursive: true });
if ((await readdir(output)).length) throw new Error("Output must be empty");
const checksums = [];
const platforms = {};
const platformNames = {
  ".exe": "windows-x86_64-nsis",
  ".msi": "windows-x86_64-msi",
  ".AppImage": "linux-x86_64-appimage",
};
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
  if (platformNames[extension]) {
    const signature = (await readFile(`${source}.sig`, "utf8")).trim();
    await verifyUpdateSignature(
      target,
      signature,
      tauri.plugins.updater.pubkey,
      version,
    );
    const signatureName = `${name}.sig`;
    await writeFile(path.join(output, signatureName), signature + "\n");
    checksums.push(
      `${createHash("sha256")
        .update(signature + "\n")
        .digest("hex")}  ${signatureName}`,
    );
    platforms[platformNames[extension]] = {
      signature,
      url: `https://github.com/eyotang/s3studio/releases/download/v${version}/${name}`,
    };
  }
}
const date = spawnSync("git", ["show", "-s", "--format=%cI", "HEAD"], {
  encoding: "utf8",
});
if (date.status !== 0 || Number.isNaN(Date.parse(date.stdout.trim())))
  throw new Error("Cannot determine release date");
const manifest =
  JSON.stringify(
    {
      version,
      notes: await readFile(`docs/releases/${version}.md`, "utf8"),
      pub_date: date.stdout.trim(),
      platforms,
    },
    null,
    2,
  ) + "\n";
await writeFile(path.join(output, "latest.json"), manifest);
checksums.push(
  `${createHash("sha256").update(manifest).digest("hex")}  latest.json`,
);
await writeFile(
  path.join(output, "SHA256SUMS.txt"),
  checksums.join("\n") + "\n",
);
