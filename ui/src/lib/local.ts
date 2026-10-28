import type { LocalState } from "./types";

export function localInteger(raw: string): number | null {
  if (!/^\d+$/.test(raw.trim())) return null;
  const n = Number(raw);
  return Number.isInteger(n) && n >= 0 && n <= 65535 ? n : null;
}

export function localStatus(snapshot: LocalState | null, model: string, build: string) {
  if (snapshot?.progress) return "local.downloading";
  if (snapshot?.running_model === model && snapshot.running_build === build) return "local.running";
  if (snapshot?.installed.includes(model) && snapshot.installed.includes(build)) return "local.ready";
  return "local.not_installed";
}
