import type { S3Object } from "./types";
export function formatBytes(value: number): string {
  if (!value) return "0 B";
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  return `${(value / 1024 ** index).toLocaleString("zh-CN", { maximumFractionDigits: index ? 1 : 0 })} ${["B", "KB", "MB", "GB", "TB"][index]}`;
}
export function objectName(key: string, prefix: string): string {
  return key.slice(prefix.length).replace(/\/$/, "");
}
export function filterObjects(
  objects: S3Object[],
  query: string,
  sort: "name" | "size" | "modified",
  ascending: boolean,
): S3Object[] {
  return objects
    .filter((o) =>
      o.key.toLocaleLowerCase().includes(query.toLocaleLowerCase()),
    )
    .sort((a, b) => {
      if (a.folder !== b.folder) return a.folder ? -1 : 1;
      const result =
        sort === "size"
          ? a.size - b.size
          : sort === "modified"
            ? a.modified.localeCompare(b.modified)
            : a.key.localeCompare(b.key, "zh-CN", { numeric: true });
      return ascending ? result : -result;
    });
}
