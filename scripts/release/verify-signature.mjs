// Verify Tauri's base64-wrapped Minisign signature using Node's Ed25519 and BLAKE2b.
// Format: https://jedisct1.github.io/minisign/ (also used by minisign-verify in the updater).
import { createHash, createPublicKey, verify } from "node:crypto";

function decode(value) {
  const text = value.trim();
  if (!text || !/^[A-Za-z0-9+/]+={0,2}$/.test(text) || text.length % 4 !== 0) {
    throw new Error("invalid signature/key encoding");
  }
  const bytes = Buffer.from(text, "base64");
  if (bytes.toString("base64") !== text) throw new Error("invalid signature/key encoding");
  return bytes;
}

/** Verifies the installer AND the trusted comment, then requires the expected signed version. */
export function verifySignature(data, encodedSignature, encodedPublicKey, version) {
  const keyLines = decode(encodedPublicKey).toString("utf8").trim().split(/\r?\n/);
  const lines = decode(encodedSignature).toString("utf8").trim().split(/\r?\n/);
  if (keyLines.length !== 2 || lines.length !== 4 || !lines[2].startsWith("trusted comment: ")) {
    throw new Error("invalid signature/key encoding");
  }
  const key = decode(keyLines[1]);
  const packet = decode(lines[1]);
  const global = decode(lines[3]);
  if (key.length !== 42 || packet.length !== 74 || global.length !== 64) {
    throw new Error("invalid signature/key encoding");
  }
  const algorithm = packet.subarray(0, 2).toString("ascii");
  if (!["Ed", "ED"].includes(algorithm) || !["Ed", "ED"].includes(key.subarray(0, 2).toString("ascii"))) {
    throw new Error("unsupported signature algorithm");
  }
  if (!packet.subarray(2, 10).equals(key.subarray(2, 10))) throw new Error("signature key does not match the app");
  const publicKey = createPublicKey({
    // RFC 8410 SubjectPublicKeyInfo prefix for a raw Ed25519 public key.
    key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), key.subarray(10)]),
    format: "der", type: "spki",
  });
  const signature = packet.subarray(10);
  const body = algorithm === "ED" ? createHash("blake2b512").update(data).digest() : data;
  const comment = lines[2].slice("trusted comment: ".length);
  if (!verify(null, body, publicKey, signature)
      || !verify(null, Buffer.concat([signature, Buffer.from(comment)]), publicKey, global)) {
    throw new Error("installer or trusted comment signature verification failed");
  }
  if (version !== undefined) {
    const versions = comment.split(/\s+/).filter((part) => part.startsWith("version:"));
    if (versions.length !== 1 || versions[0] !== `version:${version}`) {
      throw new Error("signed version does not match the release version");
    }
  }
}
