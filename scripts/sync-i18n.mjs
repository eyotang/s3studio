// Keep compile-time format macros aligned with the editable language catalogs.
import { readFileSync, writeFileSync } from "node:fs";
const languages = {
  en: "English",
  ko: "Korean",
  ja: "Japanese",
  fr: "French",
  de: "German",
  ru: "Russian",
};
const catalogs = Object.fromEntries(
  Object.keys(languages).map((code) => [
    code,
    JSON.parse(readFileSync(`src-core/locales/${code}.json`, "utf8")),
  ]),
);
const file = "src-core/src/i18n.rs";
let source = readFileSync(file, "utf8");
const start = source.indexOf("#[macro_export]");
const end = source.indexOf("fn settings_path()", start);
const patterns = [
  ...source
    .slice(start, end)
    .matchAll(/\(\s*("(?:[^"\\]|\\.)*")\s*,\s*\$\(\$args:tt\)\*/g),
].map((m) => JSON.parse(m[1]));
const placeholder = (text) =>
  [...text.matchAll(/\{[^{}]*\}/g)]
    .map((m) => m[0])
    .sort()
    .join("|");
for (const [code, catalog] of Object.entries(catalogs)) {
  if (
    JSON.stringify(Object.keys(catalog).sort()) !==
    JSON.stringify(Object.keys(catalogs.en).sort())
  )
    throw new Error(`Missing keys: ${code}`);
  for (const key of Object.keys(catalog)) {
    if (placeholder(key) !== placeholder(catalog[key]))
      throw new Error(`Invalid placeholders: ${code}: ${key}`);
  }
}
const macro =
  "#[macro_export]\nmacro_rules! lf {\n" +
  patterns
    .map((key) => {
      const arms = Object.entries(languages)
        .map(
          ([code, name]) =>
            `$crate::i18n::Language::${name} => format!(${JSON.stringify(catalogs[code][key])}, $($args)*),`,
        )
        .join("\n");
      return `(${JSON.stringify(key)}, $($args:tt)*) => { match $crate::i18n::current() {\n${arms}\n_ => format!(${JSON.stringify(key)}, $($args)*), } };`;
    })
    .join("\n") +
  "\n}\n\n";
writeFileSync(file, source.slice(0, start) + macro + source.slice(end));
console.log(
  `Validated ${Object.keys(catalogs.en).length} messages in ${Object.keys(languages).length} catalogs; synchronized ${patterns.length} format macros.`,
);
