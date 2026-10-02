#!/usr/bin/env node
// Release helper for .github/workflows/release.yml and the local release build.
//
//   node scripts/release/latest-json.mjs check-version <tag>
//   node scripts/release/latest-json.mjs assets <tag> <nsis-bundle-dir> <out-dir>
//
// `assets` copies the signed NSIS installer and its .sig under a fixed ASCII name (GitHub turns
// spaces in asset names into dots, which would break the manifest URL) and writes latest.json,
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

export function assetName(version) {
  return `opit-speech-to-text_${version}_x64-setup.exe`;
}

export function downloadUrl(version) {
  return `https://github.com/${REPO}/releases/download/v${version}/${assetName(version)}`;
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
export function manifest({ version, signature, pubDate }) {
  const sig = signature.trim();
  if (!sig) throw new Error("the signature is empty");
  return {
    version,
    notes: `https://github.com/${REPO}/releases/tag/v${version}`,
    pub_date: pubDate,
    platforms: { "windows-x86_64": { signature: sig, url: downloadUrl(version) } },
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

export function releaseFiles(version) {
  return [assetName(version), `${assetName(version)}.sig`, "latest.json", "SHA256SUMS"];
}

function checksums(dir, version) {
  return releaseFiles(version).slice(0, 3).map((name) =>
    `${createHash("sha256").update(readFileSync(join(dir, name))).digest("hex")}  ${name}`,
  ).join("\n") + "\n";
}

function verifyInstaller(data, signature, version) {
  const conf = JSON.parse(readFileSync(join(ROOT, "crates/app/tauri.conf.json"), "utf8"));
  verifySignature(data, signature, conf.plugins.updater.pubkey, version);
}

export function assets(tag, bundleDir, outDir) {
  const version = checkVersion(tag);
  const installer = findInstaller(readdirSync(bundleDir), version);
  const signature = readFileSync(join(bundleDir, `${installer}.sig`), "utf8");
  const data = readFileSync(join(bundleDir, installer));
  verifyInstaller(data, signature, version);
  mkdirSync(outDir, { recursive: true });
  copyFileSync(join(bundleDir, installer), join(outDir, assetName(version)));
  writeFileSync(join(outDir, `${assetName(version)}.sig`), signature);
  const json = manifest({ version, signature, pubDate: rfc3339(new Date()) });
  writeFileSync(join(outDir, "latest.json"), `${JSON.stringify(json, null, 2)}\n`);
  writeFileSync(join(outDir, "SHA256SUMS"), checksums(outDir, version));
  console.log(`release assets for v${version} written to ${outDir}`);
}

export function verifyAssets(tag, dir) {
  const version = checkVersion(tag);
  const data = readFileSync(join(dir, assetName(version)));
  const signature = readFileSync(join(dir, `${assetName(version)}.sig`), "utf8");
  verifyInstaller(data, signature, version);
  const json = JSON.parse(readFileSync(join(dir, "latest.json"), "utf8"));
  const expected = manifest({ version, signature, pubDate: json.pub_date });
  if (!json.pub_date || Number.isNaN(Date.parse(json.pub_date))
      || rfc3339(new Date(json.pub_date)) !== json.pub_date || !isDeepStrictEqual(json, expected)) {
    throw new Error("latest.json does not match the verified installer, version and download URL");
  }
  if (readFileSync(join(dir, "SHA256SUMS"), "utf8") !== checksums(dir, version)) {
    throw new Error("release checksums do not match");
  }
  console.log(`release assets for v${version}: signature, signed version, manifest and checksums verified`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const [command, ...args] = process.argv.slice(2);
  try {
    if (command === "check-version" && args.length === 1) console.log(checkVersion(args[0]));
    else if (command === "assets" && args.length === 3) assets(args[0], args[1], args[2]);
    else if (command === "verify" && args.length === 2) verifyAssets(args[0], args[1]);
    else throw new Error("usage: latest-json.mjs check-version <tag> | assets <tag> <bundle-dir> <out-dir> | verify <tag> <assets-dir>");
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
}
