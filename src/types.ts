export interface S3Connection {
  endpoint: string;
  region: string;
}
export interface S3Defaults extends S3Connection {
  endpoints: string[];
  regions: string[];
}
export interface Account extends S3Connection {
  id: string;
  name: string;
  pathStyle: boolean;
}
export interface AccountInput extends Account {
  accessKey: string;
  secretKey: string;
  sessionToken: string;
}
export interface Bucket extends S3Connection {
  name: string;
  created: string;
}
export interface S3Object {
  key: string;
  size: number;
  modified: string;
  storageClass: string;
  folder: boolean;
}
export interface ObjectPage {
  objects: S3Object[];
  nextToken: string | null;
}
export interface Transfer {
  id: string;
  name: string;
  direction: "upload" | "download";
  status:
    | "queued"
    | "preparing"
    | "running"
    | "pausing"
    | "paused"
    | "finalizing"
    | "done"
    | "error"
    | "cancelled";
  accountId: string;
  bucket: string;
  key: string;
  bytes: number;
  total: number;
  speed: number;
  error?: string;
}
export interface TransferSettings {
  concurrency: number;
}
export interface ObjectInfo {
  size: number;
  contentType: string;
  etag: string;
  modified: string;
  metadata: Record<string, string>;
}
