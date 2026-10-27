import { readFile, writeFile, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import path from "node:path";

const directory = process.argv[2];
if (!directory) throw new Error("Usage: publish-omarchy.mjs DIRECTORY");
const { version } = JSON.parse(await readFile("package.json", "utf8"));
const tag = `v${version}`;
const name = `S3-Studio_${version}_omarchy_x86_64.pkg.tar.zst`;
const names = [name, "SHA256SUMS-Omarchy.txt"];
function gh(args) {
  const result = spawnSync("gh", args, { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr);
  return result.stdout;
}
const release = JSON.parse(
  gh(["release", "view", tag, "--json", "isDraft,isPrerelease,body,assets"]),
);
if (release.isDraft || release.isPrerelease)
  throw new Error("Expected an existing stable release");
if (release.assets.some((asset) => names.includes(asset.name)))
  throw new Error(
    "Omarchy assets already exist; refusing to overwrite published files",
  );
const files = names.map((file) => path.join(directory, file));
const sizes = await Promise.all(
  files.map(async (file) => (await stat(file)).size),
);
if (sizes.some((size) => !size)) throw new Error("Empty Omarchy asset");
gh(["release", "upload", tag, ...files]);
const { assets } = JSON.parse(gh(["release", "view", tag, "--json", "assets"]));
for (const [i, file] of names.entries()) {
  if (!assets.some((asset) => asset.name === file && asset.size === sizes[i]))
    throw new Error(`Uploaded asset mismatch: ${file}`);
}
const notes = `${release.body.trimEnd()}\n\n## Omarchy / Arch Linux x86_64\n\n新增原生 pacman 安装包，已验证 Arch 环境中的安装、依赖、X11 / Wayland 窗口启动及卸载。\n\n下载 \`${name}\` 和 \`SHA256SUMS-Omarchy.txt\`，在下载目录执行：\n\n\`\`\`bash\nsha256sum --ignore-missing -c SHA256SUMS-Omarchy.txt\nsudo pacman -U ./${name}\n\`\`\`\n\n安装后从应用启动器打开 **S3 Studio**。凭据需要已解锁的 Secret Service 密钥库；文件选择使用桌面提供的 portal。此包尚未加入 AUR 或 Omarchy 仓库，后续更新下载新包后仍使用 \`pacman -U\` 安装；应用内沿用原有下载页更新方式。\n`;
const notesPath = path.join(directory, "release-notes.md");
await writeFile(notesPath, notes);
gh(["release", "edit", tag, "--notes-file", notesPath]);
console.log(
  gh(["release", "view", tag, "--json", "url", "--jq", ".url"]).trim(),
);
