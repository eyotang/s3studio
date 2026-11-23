import { spawnSync } from "node:child_process";
import { mkdir, mkdtemp, rm } from "node:fs/promises";
import path from "node:path";
import { appConfig, checkApplicationVersions } from "./app-config.mjs";

await checkApplicationVersions();
const stage = path.resolve("src-nsh/target/installer");
const output = path.join(stage, "bundle");
await mkdir(output, { recursive: true });
const work = await mkdtemp(path.join(stage, "package-"));
const env = {
  ...process.env,
  S3_STUDIO_VERSION: appConfig.version,
  S3_STUDIO_NAME: appConfig.productName,
  S3_STUDIO_ID: appConfig.identifier,
  S3_STUDIO_MACOS_MINIMUM: appConfig.minimumMacOS,
  S3_STUDIO_MACOS_CERTIFICATE_SHA256: appConfig.macOSSigningCertificateSha256,
  S3_STUDIO_MACOS_TEAM_ID: appConfig.macOSTeamId ?? "",
};
const platforms = {
  linux: [
    "bash", "packaging/linux/package.sh",
    path.join(stage, "release/s3-studio"),
  ],
  darwin: [
    "bash", "packaging/macos/package.sh",
    path.join(stage, "universal-apple-darwin/release/s3-studio"),
  ],
  win32: [
    "pwsh", "-NoProfile", "-File", "packaging/windows/package.ps1",
    path.join(stage, "release/s3-studio.exe"),
  ],
};

try {
  const platform = platforms[process.platform];
  if (!platform) throw new Error(`Unsupported installer platform: ${process.platform}`);
  const [program, ...args] = platform;
  const result = spawnSync(program, [...args, work, output], {
    stdio: "inherit",
    env,
  });
  if (result.error) throw result.error;
  process.exitCode = result.status ?? 1;
} finally {
  await rm(work, { recursive: true, force: true });
}
