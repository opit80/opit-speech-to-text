import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { test } from "node:test";
import { verifySignature } from "./verify-signature.mjs";

const encode = (text) => Buffer.from(text).toString("base64");

function fixture(algorithm = "ED", version = "0.1.2") {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const id = Buffer.from("12345678");
  const rawKey = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
  const pubkey = encode(`untrusted comment: test key\n${Buffer.concat([Buffer.from("Ed"), id, rawKey]).toString("base64")}\n`);
  const data = Buffer.from("throwaway release test data");
  const body = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  const sig = sign(null, body, privateKey);
  const comment = `timestamp:1\tfile:test.exe\tversion:${version}`;
  const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), privateKey);
  const signature = encode(`untrusted comment: test signature\n${Buffer.concat([Buffer.from(algorithm), id, sig]).toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`);
  return { data, signature, pubkey };
}

test("verifies both Tauri-supported minisign algorithms and the signed version", () => {
  for (const algorithm of ["ED", "Ed"]) {
    const f = fixture(algorithm);
    assert.doesNotThrow(() => verifySignature(f.data, f.signature, f.pubkey, "0.1.2"));
  }
});

test("rejects another key, modified installer, modified comment and wrong version", () => {
  const f = fixture();
  assert.throws(() => verifySignature(f.data, f.signature, fixture().pubkey, "0.1.2"), /signature/i);
  assert.throws(() => verifySignature(Buffer.from("tampered"), f.signature, f.pubkey, "0.1.2"), /signature/i);
  const tampered = encode(Buffer.from(f.signature, "base64").toString("utf8").replace("version:0.1.2", "version:0.1.3"));
  assert.throws(() => verifySignature(f.data, tampered, f.pubkey, "0.1.3"), /signature/i);
  assert.throws(() => verifySignature(f.data, f.signature, f.pubkey, "0.1.3"), /version/i);
});

test("rejects invalid encoding, unknown algorithms and a missing signed version", () => {
  const f = fixture();
  assert.throws(() => verifySignature(f.data, "not base64!", f.pubkey, "0.1.2"), /encoding/i);
  const unknown = encode(Buffer.from(f.signature, "base64").toString("utf8").replace(/^RUQ/m, "WFg"));
  assert.throws(() => verifySignature(f.data, unknown, f.pubkey, "0.1.2"));
  const missing = fixture("ED", "");
  assert.throws(() => verifySignature(missing.data, missing.signature, missing.pubkey, "0.1.2"), /version/i);
});
