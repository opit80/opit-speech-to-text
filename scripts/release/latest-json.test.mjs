import assert from "node:assert/strict";
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import {
  assetName, assets, checkVersion, downloadUrl, findInstaller, manifest, releaseFiles, rfc3339, versionFromTag, workspaceVersion,
} from "./latest-json.mjs";

test("release tags are v<major>.<minor>.<patch>", () => {
  assert.equal(versionFromTag("v0.1.0"), "0.1.0");
  for (const bad of ["0.1.0", "v0.1", "v0.1.0-rc.1", "release-1"]) assert.throws(() => versionFromTag(bad), bad);
});

test("the app version is the one under [workspace.package]", () => {
  const toml = '[workspace]\nmembers = ["a"]\n\n[workspace.package]\nversion = "0.3.1"\nedition = "2024"\n\n[workspace.dependencies]\nversion = "9.9.9"\n';
  assert.equal(workspaceVersion(toml), "0.3.1");
  assert.equal(workspaceVersion(toml.replaceAll("\n", "\r\n")), "0.3.1");
  assert.throws(() => workspaceVersion('[package]\nversion = "1.0.0"\n'));
  const real = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8");
  assert.match(workspaceVersion(real), /^\d+\.\d+\.\d+$/);
});

test("asset names and URLs are ASCII without spaces", () => {
  assert.equal(assetName("0.1.0"), "opit-speech-to-text_0.1.0_x64-setup.exe");
  assert.equal(
    downloadUrl("0.1.0"),
    "https://github.com/opit80/opit-speech-to-text/releases/download/v0.1.0/opit-speech-to-text_0.1.0_x64-setup.exe",
  );
  assert.doesNotMatch(downloadUrl("0.1.0"), /\s/);
});

test("finds the bundler's installer for the version and insists on its signature", () => {
  const files = [
    "Opit Speech to Text_0.1.0_x64-setup.exe",
    "Opit Speech to Text_0.1.0_x64-setup.exe.sig",
    "Opit Speech to Text_0.0.9_x64-setup.exe",
  ];
  assert.equal(findInstaller(files, "0.1.0"), "Opit Speech to Text_0.1.0_x64-setup.exe");
  assert.throws(() => findInstaller(files.slice(0, 1), "0.1.0"), /\.sig is missing/);
  assert.throws(() => findInstaller(files, "0.2.0"), /found 0/);
});

test("latest.json uses the updater's static format", () => {
  const json = manifest({ version: "0.1.0", signature: "c2lnbmF0dXJl\n", pubDate: "2026-10-01T12:00:00Z" });
  assert.deepEqual(json, {
    version: "0.1.0",
    notes: "https://github.com/opit80/opit-speech-to-text/releases/tag/v0.1.0",
    pub_date: "2026-10-01T12:00:00Z",
    platforms: { "windows-x86_64": { signature: "c2lnbmF0dXJl", url: downloadUrl("0.1.0") } },
  });
  assert.throws(() => manifest({ version: "0.1.0", signature: " \n", pubDate: "x" }), /empty/);
});

test("pub_date is RFC 3339 without milliseconds", () => {
  assert.equal(rfc3339(new Date(Date.UTC(2026, 9, 1, 12, 0, 0, 123))), "2026-10-01T12:00:00Z");
});

test("refuses version mismatches before building or writing release assets", () => {
  const version = workspaceVersion(readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8"));
  assert.equal(checkVersion(`v${version}`), version);
  assert.throws(() => checkVersion("v999.999.999"), /does not match/);
});

test("a bogus signature never produces a manifest or copies an installer", () => {
  const version = workspaceVersion(readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8"));
  const base = resolve(tmpdir());
  const dir = mkdtempSync(join(base, "opit-release-test-"));
  try {
    const bundle = join(dir, "bundle");
    const out = join(dir, "out");
    mkdirSync(bundle);
    writeFileSync(join(bundle, assetName(version)), "not an installer");
    writeFileSync(join(bundle, `${assetName(version)}.sig`), "not a signature");
    assert.throws(() => assets(`v${version}`, bundle, out), /encoding/i);
    assert.equal(existsSync(out), false);
    assert.deepEqual(releaseFiles(version), [assetName(version), `${assetName(version)}.sig`, "latest.json", "SHA256SUMS"]);
  } finally {
    assert.equal(dirname(dir), base, "cleanup stays within the allocated temporary directory");
    rmSync(dir, { recursive: true });
  }
});
