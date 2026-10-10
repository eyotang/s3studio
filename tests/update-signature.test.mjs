import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { verifyUpdateSignature } from "../scripts/update-signature.mjs";

test("updater verifies payload, trusted version and configured public key", async () => {
  const directory = await mkdtemp(path.join(tmpdir(), "s3studio-signature-"));
  try {
    const file = path.join(directory, "installer.exe");
    const bytes = Buffer.from("Test installer payload");
    await writeFile(file, bytes);
    const { publicKey, privateKey } = generateKeyPairSync("ed25519");
    const keyId = Buffer.from("0102030405060708", "hex");
    const key = Buffer.concat([
      Buffer.from("Ed"),
      keyId,
      publicKey.export({ type: "spki", format: "der" }).subarray(-32),
    ]);
    const encodedKey = Buffer.from(
      `untrusted comment: test\n${key.toString("base64")}\n`,
    ).toString("base64");
    const signature = sign(
      null,
      createHash("blake2b512").update(bytes).digest(),
      privateKey,
    );
    const packet = Buffer.concat([Buffer.from("ED"), keyId, signature]);
    const trusted = "timestamp:1\tfile:installer.exe\tversion:0.2.3";
    const global = sign(
      null,
      Buffer.concat([signature, Buffer.from(trusted)]),
      privateKey,
    );
    const plain = `untrusted comment: test\n${packet.toString("base64")}\ntrusted comment: ${trusted}\n${global.toString("base64")}\n`;
    const encoded = Buffer.from(plain).toString("base64");
    await verifyUpdateSignature(file, encoded, encodedKey, "0.2.3");
    await assert.rejects(
      verifyUpdateSignature(file, encoded, encodedKey, "0.2.4"),
    );
    await assert.rejects(
      verifyUpdateSignature(
        file,
        Buffer.from(plain.replace("version:0.2.3", "version:0.2.4")).toString(
          "base64",
        ),
        encodedKey,
        "0.2.4",
      ),
    );
    const wrong = Buffer.from(encodedKey, "base64").toString().split("\n");
    const otherKey = Buffer.from(key);
    otherKey[10] ^= 1;
    wrong[1] = otherKey.toString("base64");
    await assert.rejects(
      verifyUpdateSignature(
        file,
        encoded,
        Buffer.from(wrong.join("\n")).toString("base64"),
        "0.2.3",
      ),
    );
    await writeFile(file, Buffer.from("Changed installer"));
    await assert.rejects(
      verifyUpdateSignature(file, encoded, encodedKey, "0.2.3"),
    );
    await assert.rejects(
      verifyUpdateSignature(file, "invalid", encodedKey, "0.2.3"),
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
