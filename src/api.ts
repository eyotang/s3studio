import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  Account,
  Bucket,
  ObjectPage,
  S3Connection,
  S3Defaults,
  Transfer,
} from "./types";
export const native = isTauri();
let demoConcurrency = 8;
let demoS3Defaults: S3Defaults = {
  endpoint: "",
  region: "",
  endpoints: [],
  regions: [],
};
let demoTransfers: Transfer[] =
  typeof window !== "undefined" &&
  new URLSearchParams(window.location.search).has("previewTransfers")
    ? [
        {
          id: "preview-upload",
          name: "Summer collection.zip",
          direction: "upload",
          status: "paused",
          bytes: 16777216,
          total: 48700000,
          speed: 0,
          accountId: "demo-aws",
          bucket: "studio-assets",
          key: "Summer collection.zip",
        },
        {
          id: "preview-download",
          name: "Photography archive.zip",
          direction: "download",
          status: "running",
          bytes: 67108864,
          total: 134217728,
          speed: 14680064,
          accountId: "demo-aws",
          bucket: "studio-assets",
          key: "Photography archive.zip",
        },
      ]
    : [];
const demoAccounts: Account[] = [
  {
    id: "demo-aws",
    name: "Design workspace",
    region: "ap-southeast-1",
    endpoint: "",
    pathStyle: false,
  },
  {
    id: "demo-minio",
    name: "Local development",
    region: "us-east-1",
    endpoint: "http://localhost:9000",
    pathStyle: true,
  },
];
const demoBuckets: Bucket[] = [
  { name: "studio-assets", created: "2026-08-16T08:00:00Z" },
  { name: "application-backups", created: "2026-07-10T08:00:00Z" },
  { name: "team-documents", created: "2026-06-12T08:00:00Z" },
].map((bucket) => ({ ...bucket, endpoint: "", region: "" }));
const demoConnections = new Map<string, S3Connection>();
const demoItems = [
  { name: "Brand assets/", size: 0 },
  { name: "Photography/", size: 0 },
  { name: "Product design/", size: 0 },
  { name: "Brand guidelines.pdf", size: 2488000 },
  { name: "Homepage exploration.fig", size: 18400000 },
  { name: "Mountain retreat.jpg", size: 6720000 },
  { name: "Project manifest.json", size: 2140 },
  { name: "Summer collection.zip", size: 48700000 },
  { name: "Welcome.md", size: 1820 },
];
export async function api<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (native) return invoke<T>(command, args);
  await new Promise((resolve) => setTimeout(resolve, 180));
  if (command === "list_accounts") return demoAccounts as T;
  if (command === "get_s3_defaults") return demoS3Defaults as T;
  if (command === "set_s3_defaults") {
    const input = args.input as S3Defaults;
    demoS3Defaults = { ...input };
    for (const field of ["endpoint", "region"] as const) {
      if (input[field])
        demoS3Defaults[`${field}s`] = [
          ...new Set([...input[`${field}s`], input[field]]),
        ].sort();
    }
    return demoS3Defaults as T;
  }
  if (command === "add_s3_option") {
    const field = args.field as keyof S3Connection;
    const options = `${field}s` as const;
    demoS3Defaults = {
      ...demoS3Defaults,
      [options]: [
        ...new Set([...demoS3Defaults[options], String(args.value)]),
      ].sort(),
    };
    return demoS3Defaults as T;
  }
  if (command === "set_bucket_connection") {
    demoConnections.set(
      JSON.stringify([args.accountId, args.bucket]),
      args.connection as S3Connection,
    );
    return args.connection as T;
  }
  if (command === "list_transfers") return demoTransfers as T;
  if (command === "get_transfer_settings")
    return { concurrency: demoConcurrency } as T;
  if (command === "set_transfer_settings") {
    const concurrency = Number(args.concurrency);
    if (!Number.isInteger(concurrency) || concurrency < 1 || concurrency > 32)
      throw new Error("并发数量需为 1–32 的整数。");
    demoConcurrency = concurrency;
    return { concurrency } as T;
  }
  if (
    ["pause_transfer", "resume_transfer", "cancel_transfer"].includes(
      command,
    ) &&
    new URLSearchParams(window.location.search).has("previewTransfers")
  ) {
    demoTransfers = demoTransfers.map((t) =>
      t.id === args.transferId
        ? {
            ...t,
            status:
              command === "pause_transfer"
                ? "paused"
                : command === "resume_transfer"
                  ? "running"
                  : "cancelled",
            speed: 0,
          }
        : t,
    );
    return undefined as T;
  }
  if (command === "clear_finished_transfers") {
    demoTransfers = demoTransfers.filter(
      (t) => !["done", "cancelled"].includes(t.status),
    );
    return undefined as T;
  }
  if (command === "list_buckets")
    return demoBuckets.map((bucket) => ({
      ...bucket,
      ...demoConnections.get(JSON.stringify([args.accountId, bucket.name])),
    })) as T;
  if (command === "list_objects") {
    const prefix = (args.prefix as string) || "";
    const items = prefix
      ? [
          { name: "Readme.md", size: 1024 },
          { name: "Reference.png", size: 254000 },
        ]
      : demoItems;
    return {
      objects: items.map((item, i) => ({
        key: prefix + item.name,
        size: item.size,
        folder: item.name.endsWith("/"),
        modified: `2026-10-0${9 - (i % 5)}T08:30:00Z`,
        storageClass: "STANDARD",
      })),
      nextToken: null,
    } as ObjectPage as T;
  }
  if (command === "object_info")
    return {
      size: 2488000,
      contentType: "application/octet-stream",
      etag: "preview-object",
      modified: "2026-10-09T08:30:00Z",
      metadata: {},
    } as T;
  throw new Error(
    "当前是界面预览。请运行 npm run tauri dev，在桌面应用中连接真实账户。",
  );
}
