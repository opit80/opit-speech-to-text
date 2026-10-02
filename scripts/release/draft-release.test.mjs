import assert from "node:assert/strict";
import { test } from "node:test";
import { draftCommands } from "./draft-release.mjs";

test("creates an unpublished non-prerelease draft with only the four expected assets", () => {
  const commands = draftCommands(null, "v0.1.2", "release-assets");
  assert.equal(commands.length, 1);
  assert.deepEqual(commands[0].slice(0, 3), ["release", "create", "v0.1.2"]);
  assert.ok(commands[0].includes("--draft"));
  assert.ok(commands[0].includes("--latest=false"));
  assert.ok(commands[0].includes("--verify-tag"));
  assert.ok(commands[0].includes("--repo"));
  assert.equal(commands[0].filter((arg) => arg.startsWith("release-assets")).length, 4);
  assert.ok(commands[0].every((arg) => !arg.includes("*")));
});

test("reruns replace draft assets without creating a second release or publishing", () => {
  const commands = draftCommands({ draft: true }, "v0.1.2", "release-assets");
  assert.deepEqual(commands[0].slice(0, 3), ["release", "upload", "v0.1.2"]);
  assert.ok(commands[0].includes("--clobber"));
  assert.ok(commands[1].includes("--draft=true"));
  assert.ok(commands[1].includes("--latest=false"));
  assert.ok(commands[1].includes("--prerelease=false"));
});

test("published releases and malformed release responses are never overwritten", () => {
  for (const release of [{ draft: false }, {}, { draft: "true" }]) {
    assert.throws(() => draftCommands(release, "v0.1.2", "release-assets"), /draft/i);
  }
});
