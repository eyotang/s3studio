import { spawnSync } from "node:child_process";
import { appendFile, readFile, readdir, stat } from "node:fs/promises";
import path from "node:path";

const [directory] = process.argv.slice(2);
if (!directory) throw new Error("Usage: publish-release.mjs DIRECTORY");
const { version } = JSON.parse(await readFile("package.json", "utf8"));
const tag = `v${version}`;
if (process.env.GITHUB_REF_NAME !== tag)
  throw new Error("Release tag mismatch");
const files = (await readdir(directory)).sort();
if (
  files.length !== 11 ||
  !files.includes("SHA256SUMS.txt") ||
  !files.includes("latest.json")
)
  throw new Error(
    "Expected six installers, three signatures, latest.json and SHA256SUMS.txt",
  );
const manifest = JSON.parse(
  await readFile(path.join(directory, "latest.json"), "utf8"),
);
if (
  manifest.version !== version ||
  Object.keys(manifest.platforms).length !== 3
)
  throw new Error("Updater manifest does not match release");
const expected = new Map();
for (const file of files)
  expected.set(file, (await stat(path.join(directory, file))).size);

function gh(args, optional = false) {
  const result = spawnSync("gh", args, { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0 && !optional)
    throw new Error(result.stderr || `gh exited ${result.status}`);
  return result;
}

const current = gh(["release", "view", tag, "--json", "isDraft"], true);
if (current.status === 0) {
  if (!JSON.parse(current.stdout).isDraft)
    throw new Error(
      "Published release already exists; refusing to overwrite it",
    );
} else {
  gh([
    "release",
    "create",
    tag,
    "--verify-tag",
    "--draft",
    "--title",
    `S3 Studio ${version}`,
    "--notes-file",
    `docs/releases/${version}.md`,
  ]);
}
gh([
  "release",
  "upload",
  tag,
  ...files.map((file) => path.join(directory, file)),
  "--clobber",
]);
const remote = JSON.parse(
  gh(["release", "view", tag, "--json", "assets"]).stdout,
).assets;
if (
  remote.length !== expected.size ||
  remote.some((asset) => expected.get(asset.name) !== asset.size)
)
  throw new Error("Remote release assets do not match the verified installers");
const latest = gh(["release", "view", "--json", "tagName"], true);
const previous =
  latest.status === 0
    ? JSON.parse(latest.stdout)
        .tagName.match(/^v(\d+)\.(\d+)\.(\d+)$/)
        ?.slice(1)
        .map(Number)
    : undefined;
const parts = version.split(".").map(Number);
const newer =
  !previous ||
  parts.some(
    (part, i) =>
      part > previous[i] &&
      parts.slice(0, i).every((v, n) => v === previous[n]),
  ) ||
  parts.every((v, i) => v === previous[i]);
gh([
  "release",
  "edit",
  tag,
  "--draft=false",
  newer ? "--latest" : "--latest=false",
]);
const url = gh([
  "release",
  "view",
  tag,
  "--json",
  "url",
  "--jq",
  ".url",
]).stdout.trim();
console.log(url);
if (process.env.GITHUB_STEP_SUMMARY)
  await appendFile(
    process.env.GITHUB_STEP_SUMMARY,
    `### Published\n[${tag}](${url})\n\n${files.map((file) => `- ${file}`).join("\n")}\n`,
  );
