import assert from "node:assert/strict";
import { createHash, generateKeyPairSync, sign } from "node:crypto";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { assetName, assets, debName, linuxAssets, mergeAssets, releaseFiles, verifyAssets, workspaceVersion } from "./latest-json.mjs";

// Disposable in-memory test keys, unrelated to the application's updater signing key.
function testSigner() {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const id = Buffer.from("testonly");
  const encode = (value) => Buffer.from(value).toString("base64");
  const pubkey = encode(`untrusted comment: test key\n${Buffer.concat([Buffer.from("Ed"), id, publicKey.export({ format: "der", type: "spki" }).subarray(-32)]).toString("base64")}\n`);
  return { pubkey, signature(data, version) {
    const sig = sign(null, createHash("blake2b512").update(data).digest(), privateKey);
    const comment = `timestamp:1\tfile:fixture\tversion:${version}`;
    const global = sign(null, Buffer.concat([sig, Buffer.from(comment)]), privateKey);
    return encode(`untrusted comment: test signature\n${Buffer.concat([Buffer.from("ED"), id, sig]).toString("base64")}\ntrusted comment: ${comment}\n${global.toString("base64")}\n`);
  } };
}

function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), "opit-linux-release-test-"));
  try {
    const version = workspaceVersion(readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8"));
    const signer = testSigner();
    const imageDir = join(root, "image");
    const debDir = join(root, "deb");
    const nsisDir = join(root, "nsis");
    for (const dir of [imageDir, debDir, nsisDir]) mkdirSync(dir);
    const data = Buffer.from("disposable Linux AppImage fixture");
    const image = `Opit Speech to Text_${version}_amd64.AppImage`;
    writeFileSync(join(imageDir, image), data);
    writeFileSync(join(imageDir, `${image}.sig`), signer.signature(data, version));
    writeFileSync(join(debDir, debName(version)), "disposable deb fixture");
    const exe = Buffer.from("disposable Windows installer fixture");
    writeFileSync(join(nsisDir, assetName(version)), exe);
    writeFileSync(join(nsisDir, `${assetName(version)}.sig`), signer.signature(exe, version));
    run({ root, version, tag: `v${version}`, signer, imageDir, debDir, nsisDir, image });
  } finally {
    rmSync(root, { recursive: true });
  }
}

test("prepares, verifies and merges signed Windows and Linux artifacts into one release", () => fixture((f) => {
  const linux = join(f.root, "linux-out");
  const windows = join(f.root, "windows-out");
  const merged = join(f.root, "merged");
  const options = { pubkey: f.signer.pubkey };
  linuxAssets(f.tag, f.imageDir, f.debDir, linux, options);
  assets(f.tag, f.nsisDir, windows, options);
  assert.deepEqual(Object.keys(verifyAssets(f.tag, linux, options).platforms), ["linux-x86_64"]);
  assert.deepEqual(Object.keys(verifyAssets(f.tag, windows, options).platforms), ["windows-x86_64"]);
  mergeAssets(f.tag, windows, linux, merged, options);
  const json = verifyAssets(f.tag, merged, options);
  assert.deepEqual(Object.keys(json.platforms), ["windows-x86_64", "linux-x86_64"]);
  assert.equal(readFileSync(join(merged, "SHA256SUMS"), "utf8").trim().split("\n").length, 6);
  for (const file of releaseFiles(f.version, Object.keys(json.platforms))) assert.ok(existsSync(join(merged, file)));
}));

test("tampered AppImage, deb, manifest URL and unknown platform are refused", () => fixture((f) => {
  const out = join(f.root, "out");
  const options = { pubkey: f.signer.pubkey };
  linuxAssets(f.tag, f.imageDir, f.debDir, out, options);
  const imagePath = join(out, assetName(f.version, "linux-x86_64"));
  const imageData = readFileSync(imagePath);
  writeFileSync(imagePath, "tampered image");
  assert.throws(() => verifyAssets(f.tag, out, options), /signature/i);
  writeFileSync(imagePath, imageData);
  const debPath = join(out, debName(f.version));
  const debData = readFileSync(debPath);
  writeFileSync(debPath, "tampered deb");
  assert.throws(() => verifyAssets(f.tag, out, options), /checksums/i);
  writeFileSync(debPath, debData);
  const jsonPath = join(out, "latest.json");
  const json = JSON.parse(readFileSync(jsonPath, "utf8"));
  json.platforms["linux-x86_64"].url = "https://example.invalid/other.AppImage";
  writeFileSync(jsonPath, JSON.stringify(json));
  assert.throws(() => verifyAssets(f.tag, out, options), /manifest|latest.json/i);
  json.platforms["untrusted-x86_64"] = json.platforms["linux-x86_64"];
  writeFileSync(jsonPath, JSON.stringify(json));
  assert.throws(() => verifyAssets(f.tag, out, options), /platform/i);
}));

test("bad Linux signatures and missing packages never create a manifest", () => fixture((f) => {
  const out = join(f.root, "out");
  writeFileSync(join(f.imageDir, `${f.image}.sig`), "not a signature");
  assert.throws(() => linuxAssets(f.tag, f.imageDir, f.debDir, out, { pubkey: f.signer.pubkey }), /encoding/i);
  assert.equal(existsSync(out), false);
  rmSync(join(f.debDir, debName(f.version)));
  assert.throws(() => linuxAssets(f.tag, f.imageDir, f.debDir, out, { pubkey: f.signer.pubkey }), /found 0/i);
  assert.equal(existsSync(out), false);
}));
