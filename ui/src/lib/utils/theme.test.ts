import { afterEach, describe, expect, it, vi } from "vitest";
import { resolveTheme, watchTheme } from "./theme";

afterEach(() => vi.unstubAllGlobals());

describe("Opit Speech to Text theme choice", () => {
  it("follows system changes only for the system choice", () => {
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("system", true)).toBe("dark");
    for (const systemDark of [false, true]) {
      expect(resolveTheme("light", systemDark)).toBe("light");
      expect(resolveTheme("dark", systemDark)).toBe("dark");
    }
  });

  it("applies the root attribute, follows live OS changes and removes old listeners", () => {
    const listeners = new Set<() => void>();
    const media = {
      matches: false,
      addEventListener: (_event: string, listener: () => void) => listeners.add(listener),
      removeEventListener: (_event: string, listener: () => void) => listeners.delete(listener),
    };
    const root = { dataset: {} as Record<string, string> };
    vi.stubGlobal("window", { matchMedia: () => media });
    vi.stubGlobal("document", { documentElement: root });
    const stop = watchTheme("system");
    expect(root.dataset.theme).toBe("light");
    media.matches = true;
    listeners.forEach((listener) => listener());
    expect(root.dataset.theme).toBe("dark");
    stop();
    expect(listeners.size).toBe(0);
    const stopLight = watchTheme("light");
    listeners.forEach((listener) => listener());
    expect(root.dataset.theme).toBe("light");
    stopLight();
    expect(listeners.size).toBe(0);
  });
});
