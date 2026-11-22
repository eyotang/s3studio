import { readFile } from "node:fs/promises";

export const appConfig = JSON.parse(
  await readFile(new URL("../src-nsh/tauri.conf.json", import.meta.url), "utf8"),
);

export async function checkApplicationVersions() {
  for (const name of ["src-nsh", "src-core"]) {
    const file = new URL(`../${name}/Cargo.toml`, import.meta.url);
    const cargo = await readFile(file, "utf8");
    if (cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1] !== appConfig.version)
      throw new Error(`Version mismatch in ${name}/Cargo.toml`);
  }
}
