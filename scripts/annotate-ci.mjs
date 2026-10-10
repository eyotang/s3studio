import { readFile } from "node:fs/promises";

const log = await readFile(process.argv[2], "utf8");
const failures = log.indexOf("\nfailures:");
const diagnostic = (failures >= 0 ? log.slice(failures) : log)
  .split(/\r?\n/)
  .filter((line) => !/^\s*(Compiling|Finished|Running)\b/.test(line))
  .slice(-100)
  .join("\n")
  .slice(-20000);
for (let offset = 0; offset < diagnostic.length; offset += 2000) {
  const escaped = diagnostic
    .slice(offset, offset + 2000)
    .replaceAll("%", "%25")
    .replaceAll("\r", "%0D")
    .replaceAll("\n", "%0A");
  console.log(
    `::error title=Desktop build diagnostics ${offset / 2000 + 1}::${escaped}`,
  );
}
