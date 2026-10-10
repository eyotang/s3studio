import { test } from "node:test";
import assert from "node:assert/strict";
import { filterObjects, objectName, formatBytes } from "../src/utils.ts";

const items = [
  {
    key: "design/file10.png",
    folder: false,
    size: 400,
    modified: "2026-10-03",
  },
  {
    key: "design/file2.png",
    folder: false,
    size: 1024,
    modified: "2026-10-01",
  },
  { key: "design/Archive/", folder: true, size: 0, modified: "" },
];
test("directory stays first in ascending and descending sorts", () => {
  assert.deepEqual(
    filterObjects(items, "", "name", true).map((o) => o.key),
    ["design/Archive/", "design/file2.png", "design/file10.png"],
  );
  assert.deepEqual(
    filterObjects(items, "", "size", false).map((o) => o.size),
    [0, 1024, 400],
  );
  assert.equal(items[0].key, "design/file10.png");
});
test("case insensitive search and relative folder names", () => {
  assert.equal(filterObjects(items, "ARCHIVE", "name", true).length, 1);
  assert.equal(filterObjects(items, "missing", "name", true).length, 0);
  assert.equal(objectName("design/Archive/", "design/"), "Archive");
});
test("size and date sorting preserve directory priority", () => {
  assert.equal(
    filterObjects(items, "", "modified", false)[1].key,
    "design/file10.png",
  );
  assert.equal(filterObjects(items, "file", "size", true)[0].size, 400);
});
test("byte formatting handles empty files and large files", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(1024), "1 KB");
  assert.equal(formatBytes(1024 ** 4), "1 TB");
});
