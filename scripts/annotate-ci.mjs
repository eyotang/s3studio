import { readFile } from "node:fs/promises";

const log = await readFile(process.argv[2], "utf8");
const diagnostic = log.split(/\r?\n/).slice(-100).join("\n").slice(-30000);
const escaped = diagnostic
  .replaceAll("%", "%25")
  .replaceAll("\r", "%0D")
  .replaceAll("\n", "%0A");
console.log(`::error title=Desktop build diagnostics::${escaped}`);
