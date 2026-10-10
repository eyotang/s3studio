import { createHash, createPublicKey, verify } from "node:crypto";
import { createReadStream } from "node:fs";

// Verify both Minisign signatures with Node's Ed25519 implementation.
// The trusted comment binds an artifact to the version announced by latest.json.
export async function verifyUpdateSignature(
  file,
  encodedSignature,
  encodedPublicKey,
  version,
) {
  const publicLines = Buffer.from(encodedPublicKey, "base64")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  const lines = Buffer.from(encodedSignature, "base64")
    .toString("utf8")
    .trim()
    .split(/\r?\n/);
  const key = Buffer.from(publicLines[1] ?? "", "base64");
  const packet = Buffer.from(lines[1] ?? "", "base64");
  const global = Buffer.from(lines[3] ?? "", "base64");
  if (
    key.length !== 42 ||
    packet.length !== 74 ||
    global.length !== 64 ||
    packet.subarray(0, 2).toString() !== "ED" ||
    !lines[2]?.startsWith("trusted comment: ") ||
    !packet.subarray(2, 10).equals(key.subarray(2, 10))
  )
    throw new Error("Invalid updater signature or mismatched signing key");
  const comment = lines[2].slice(17);
  if (
    comment.split("\t").find((field) => field.startsWith("version:")) !==
    `version:${version}`
  )
    throw new Error("Updater signature is not bound to the release version");
  const publicKey = createPublicKey({
    key: Buffer.concat([
      Buffer.from("302a300506032b6570032100", "hex"),
      key.subarray(10),
    ]),
    format: "der",
    type: "spki",
  });
  const hash = createHash("blake2b512");
  for await (const chunk of createReadStream(file)) hash.update(chunk);
  const signature = packet.subarray(10);
  if (
    !verify(null, hash.digest(), publicKey, signature) ||
    !verify(
      null,
      Buffer.concat([signature, Buffer.from(comment)]),
      publicKey,
      global,
    )
  )
    throw new Error("Updater artifact signature verification failed");
}
