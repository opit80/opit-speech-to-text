#!/usr/bin/env node
// Release helper for .github/workflows/release.yml and the local release build.
//
//   node scripts/release/latest-json.mjs check-version <tag>
//   node scripts/release/latest-json.mjs assets <tag> <nsis-bundle-dir> <out-dir>
//
// `assets` stages Windows; `linux-assets` stages AppImage/deb; `merge-assets` combines verified inputs.
// Signed updater artifacts use fixed ASCII names (GitHub turns
// spaces in asset names into dots, which would break the manifest URL). The helper writes latest.json,
// which tauri-plugin-updater reads from
// https://github.com/opit80/opit-speech-to-text/releases/latest/download/latest.json
import { copyFileSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verifySignature } from "./verify-signature.mjs";

export const REPO = "opit80/opit-speech-to-text";
const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

/** "v1.2.3" → "1.2.3". Pre-release tags are not used in v1. */
export function versionFromTag(tag) {
  const match = /^v(\d+\.\d+\.\d+)$/.exec(tag);
  if (!match) throw new Error(`"${tag}" is not a release tag like v1.2.3`);
  return match[1];
}

/** `version` under [workspace.package] in the root Cargo.toml: the app's version. */
export function workspaceVersion(cargoToml) {
  let inSection = false;
  for (const raw of cargoToml.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("[")) {
      inSection = line === "[workspace.package]";
    } else if (inSection) {
      const match = /^version\s*=\s*"([^"]+)"$/.exec(line);
      if (match) return match[1];
    }
  }
  throw new Error("no version under [workspace.package] in Cargo.toml");
}

export function assetName(version, platform = "windows-x86_64") {
  if (platform === "windows-x86_64") return `opit-speech-to-text_${version}_x64-setup.exe`;
  if (platform === "linux-x86_64") return `opit-speech-to-text_${version}_amd64.AppImage`;
  throw new Error(`unsupported release platform: ${platform}`);
}

export function debName(version) {
  return `opit-speech-to-text_${version}_amd64.deb`;
}

export function downloadUrl(version, platform = "windows-x86_64") {
  return `https://github.com/${REPO}/releases/download/v${version}/${assetName(version, platform)}`;
}

/** RFC 3339 without milliseconds, for `pub_date`. */
export function rfc3339(date) {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

/** The bundler's `<productName>_<version>_x64-setup.exe` among `files`; its `.sig` must be there too. */
export function findInstaller(files, version) {
  const suffix = `_${version}_x64-setup.exe`;
  const found = files.filter((name) => name.endsWith(suffix));
  if (found.length !== 1) throw new Error(`expected one *${suffix} in the bundle folder, found ${found.length}`);
  if (!files.includes(`${found[0]}.sig`)) {
    throw new Error(`${found[0]}.sig is missing: build with TAURI_SIGNING_PRIVATE_KEY set`);
  }
  return found[0];
}

/** latest.json in tauri-plugin-updater's static format. */
export function manifest({ version, signature, linuxSignature, pubDate }) {
  const platforms = {};
  for (const [platform, value] of [["windows-x86_64", signature], ["linux-x86_64", linuxSignature]]) {
    if (value === undefined) continue;
    const sig = value.trim();
    if (!sig) throw new Error("the signature is empty");
    platforms[platform] = { signature: sig, url: downloadUrl(version, platform) };
  }
  if (!Object.keys(platforms).length) throw new Error("at least one platform signature is required");
  return {
    version,
    notes: `https://github.com/${REPO}/releases/tag/v${version}`,
    pub_date: pubDate,
    platforms,
  };
}

export function checkVersion(tag) {
  const version = versionFromTag(tag);
  const cargo = workspaceVersion(readFileSync(join(ROOT, "Cargo.toml"), "utf8"));
  if (version !== cargo) throw new Error(`tag ${tag} does not match the app version ${cargo} in Cargo.toml`);
  const ui = JSON.parse(readFileSync(join(ROOT, "ui/package.json"), "utf8"));
  const lock = JSON.parse(readFileSync(join(ROOT, "ui/package-lock.json"), "utf8"));
  if (ui.version !== version || lock.version !== version || lock.packages[""].version !== version) {
    throw new Error("UI package/lock version does not match the release version");
  }
  return version;
}

export function releaseFiles(version, platforms = ["windows-x86_64"]) {
  const files = [];
  for (const platform of ["windows-x86_64", "linux-x86_64"]) {
    if (!platforms.includes(platform)) continue;
    const name = assetName(version, platform);
    files.push(name, `${name}.sig`);
    if (platform === "linux-x86_64") files.push(debName(version));
  }
  if (!files.length || platforms.some((platform) => !["windows-x86_64", "linux-x86_64"].includes(platform))) {
    throw new Error("unsupported or empty release platforms");
  }
  return [...files, "latest.json", "SHA256SUMS"];
}

function checksums(dir, version, platforms) {
  return releaseFiles(version, platforms).filter((name) => name !== "SHA256SUMS").map((name) =>
    `${createHash("sha256").update(readFileSync(join(dir, name))).digest("hex")}  ${name}`,
  ).join("\n") + "\n";
}

function configuredPublicKey() {
  return JSON.parse(readFileSync(join(ROOT, "crates/app/tauri.conf.json"), "utf8")).plugins.updater.pubkey;
}

function writeManifest(outDir, json) {
  writeFileSync(join(outDir, "latest.json"), `${JSON.stringify(json, null, 2)}\n`);
  writeFileSync(join(outDir, "SHA256SUMS"), checksums(outDir, json.version, Object.keys(json.platforms)));
}

function findLinuxAsset(files, version, extension) {
  const suffix = `_${version}_amd64.${extension}`;
  const found = files.filter((name) => name.endsWith(suffix));
  if (found.length !== 1) throw new Error(`expected one *${suffix} in the bundle folder, found ${found.length}`);
  return found[0];
}

export function linuxAssets(tag, appimageDir, debDir, outDir, { pubkey = configuredPublicKey() } = {}) {
  const version = checkVersion(tag);
  const image = findLinuxAsset(readdirSync(appimageDir), version, "AppImage");
  const deb = findLinuxAsset(readdirSync(debDir), version, "deb");
  const signature = readFileSync(join(appimageDir, `${image}.sig`), "utf8");
  verifySignature(readFileSync(join(appimageDir, image)), signature, pubkey, version);
  // Read all inputs before writing anything; a failed preparation never leaves a usable manifest.
  const debData = readFileSync(join(debDir, deb));
  mkdirSync(outDir, { recursive: true });
  copyFileSync(join(appimageDir, image), join(outDir, assetName(version, "linux-x86_64")));
  writeFileSync(join(outDir, `${assetName(version, "linux-x86_64")}.sig`), signature);
  writeFileSync(join(outDir, debName(version)), debData);
  writeManifest(outDir, manifest({ version, linuxSignature: signature, pubDate: rfc3339(new Date()) }));
  console.log(`Linux release assets for v${version} written to ${outDir}`);
}

/** One aggregation step owns latest.json and SHA256SUMS, so platform jobs never race. */
export function mergeAssets(tag, windowsDir, linuxDir, outDir, options = {}) {
  const version = checkVersion(tag);
  const windows = verifyAssets(tag, windowsDir, options);
  const linux = verifyAssets(tag, linuxDir, options);
  if (!isDeepStrictEqual(Object.keys(windows.platforms), ["windows-x86_64"])
      || !isDeepStrictEqual(Object.keys(linux.platforms), ["linux-x86_64"])) {
    throw new Error("merge requires separate Windows-only and Linux-only verified assets");
  }
  mkdirSync(outDir, { recursive: true });
  for (const [dir, platforms] of [[windowsDir, ["windows-x86_64"]], [linuxDir, ["linux-x86_64"]]]) {
    for (const file of releaseFiles(version, platforms).filter((name) => !["latest.json", "SHA256SUMS"].includes(name))) {
      copyFileSync(join(dir, file), join(outDir, file));
    }
  }
  writeManifest(outDir, manifest({ version, signature: windows.platforms["windows-x86_64"].signature,
    linuxSignature: linux.platforms["linux-x86_64"].signature, pubDate: rfc3339(new Date()) }));
  verifyAssets(tag, outDir, options);
}

export function assets(tag, bundleDir, outDir, { pubkey = configuredPublicKey() } = {}) {
  const version = checkVersion(tag);
  const installer = findInstaller(readdirSync(bundleDir), version);
  const signature = readFileSync(join(bundleDir, `${installer}.sig`), "utf8");
  const data = readFileSync(join(bundleDir, installer));
  verifySignature(data, signature, pubkey, version);
  mkdirSync(outDir, { recursive: true });
  copyFileSync(join(bundleDir, installer), join(outDir, assetName(version)));
  writeFileSync(join(outDir, `${assetName(version)}.sig`), signature);
  const json = manifest({ version, signature, pubDate: rfc3339(new Date()) });
  writeManifest(outDir, json);
  console.log(`release assets for v${version} written to ${outDir}`);
}

export function verifyAssets(tag, dir, { pubkey = configuredPublicKey() } = {}) {
  const version = checkVersion(tag);
  const json = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
  const platforms = Object.keys(json.platforms ?? {});
  releaseFiles(version, platforms); // Reject unsupported and empty platform sets before reading files.
  const signatures = {};
  for (const platform of platforms) {
    const data = readFileSync(join(dir, assetName(version, platform)));
    const signature = readFileSync(join(dir, `${assetName(version, platform)}.sig`), "utf8");
    verifySignature(data, signature, pubkey, version);
    signatures[platform === "windows-x86_64" ? "signature" : "linuxSignature"] = signature;
  }
  const expected = manifest({ version, ...signatures, pubDate: json.pub_date });
  if (!json.pub_date || Number.isNaN(Date.parse(json.pub_date))
      || rfc3339(new Date(json.pub_date)) !== json.pub_date || !isDeepStrictEqual(json, expected)) {
    throw new Error("latest.json does not match the verified artifacts, version and download URLs");
  }
  if (readFileSync(join(dir, "SHA256SUMS"), "utf8") !== checksums(dir, version, platforms)) {
    throw new Error("release checksums do not match");
  }
  console.log(`release assets for v${version}: signature, signed version, manifest and checksums verified`);
  return json;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [command, ...args] = process.argv.slice(2);
  try {
    if (command === "check-version" && args.length === 1) console.log(checkVersion(args[0]));
    else if (command === "assets" && args.length === 3) assets(args[0], args[1], args[2]);
    else if (command === "linux-assets" && args.length === 4) linuxAssets(...args);
    else if (command === "merge-assets" && args.length === 4) mergeAssets(...args);
    else if (command === "verify" && args.length === 2) verifyAssets(args[0], args[1]);
    else throw new Error("usage: latest-json.mjs check-version <tag> | assets <tag> <bundle-dir> <out-dir> | linux-assets <tag> <appimage-dir> <deb-dir> <out-dir> | merge-assets <tag> <windows-dir> <linux-dir> <out-dir> | verify <tag> <assets-dir>");
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
