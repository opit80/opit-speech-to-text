// Rerunnable draft uploads. This helper never publishes a release or modifies a published one.
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { REPO, releaseFiles, verifyAssets, versionFromTag } from "./latest-json.mjs";

export function draftCommands(release, tag, dir) {
  const files = releaseFiles(versionFromTag(tag)).map((name) => join(dir, name));
  const title = `Opit Speech to Text ${tag}`;
  if (release === null) {
    return [["release", "create", tag, ...files, "--repo", REPO, "--draft", "--latest=false",
      "--verify-tag", "--generate-notes", "--title", title]];
  }
  if (release.draft !== true) throw new Error("only a draft release may be replaced; published releases are immutable here");
  return [
    ["release", "upload", tag, ...files, "--repo", REPO, "--clobber"],
    ["release", "edit", tag, "--repo", REPO, "--draft=true", "--latest=false", "--prerelease=false", "--title", title],
  ];
}

async function findRelease(tag) {
  if (!process.env.GH_TOKEN) throw new Error("GH_TOKEN is required");
  if (process.env.GITHUB_REPOSITORY && process.env.GITHUB_REPOSITORY !== REPO) {
    throw new Error("release repository does not match the updater download repository");
  }
  const response = await fetch(`https://api.github.com/repos/${REPO}/releases/tags/${encodeURIComponent(tag)}`, {
    headers: { Authorization: `Bearer ${process.env.GH_TOKEN}`, Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28" },
    signal: AbortSignal.timeout(30_000),
  });
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`release lookup failed (HTTP ${response.status})`);
  const release = await response.json();
  if (release.draft !== true) throw new Error("this tag already has a published release; use a new version");
  return release;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, tag, dir, ...extra] = process.argv.slice(2);
  try {
    versionFromTag(tag ?? "");
    if (extra.length || !["check", "upload"].includes(command) || (command === "upload") !== Boolean(dir)) {
      throw new Error("usage: draft-release.mjs check <tag> | upload <tag> <assets-dir>");
    }
    if (command === "upload") verifyAssets(tag, dir);
    const release = await findRelease(tag);
    if (command === "upload") {
      for (const args of draftCommands(release, tag, dir)) {
        const result = spawnSync("gh", args, { stdio: "inherit" });
        if (result.error || result.status !== 0) throw new Error("draft release upload failed");
      }
    }
    console.log(command === "check" ? "tag may be built as a draft" : "draft ready; publication remains manual");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
