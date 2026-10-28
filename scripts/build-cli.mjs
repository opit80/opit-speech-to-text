// Stage Opit Speech to Text's console tool as a bundle resource next to the app.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
if (process.platform !== "win32") throw new Error("The Windows bundle requires a Windows CLI build.");
const targetDir = resolve(root, process.env.CARGO_TARGET_DIR || "target/agent-build");
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
const args = ["build", "--offline", "--release", "-p", "opit-cli"];
if (target) args.push("--target", target);
const build = spawnSync("cargo", args, {
  cwd: root, stdio: "inherit",
  env: { ...process.env, CARGO_TARGET_DIR: targetDir, CARGO_NET_OFFLINE: "true", CARGO_BUILD_JOBS: "1" },
});
if (build.error || build.status !== 0) process.exit(build.status || 1);
const staged = join(root, "target", "cli-bundle");
mkdirSync(staged, { recursive: true });
copyFileSync(join(targetDir, ...(target ? [target] : []), "release", "opit.exe"), join(staged, "opit.exe"));
