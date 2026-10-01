import { describe, expect, it } from "vitest";
import { describeError, isCommandError } from "./errors";

describe("describeError", () => {
  it("prefers the localized kind", () => {
    const err = { code: "provider", message: "HTTP 401: nope", kind: "invalid_key" };
    expect(describeError(err, "tr")).toBe("API anahtarı geçersiz");
  });
  it("uses a localized prefix per code with the Rust message", () => {
    expect(describeError({ code: "rules", message: "bad yaml", kind: null }, "en")).toBe("Rules error: bad yaml");
  });
  it("handles unknown codes and non-command errors", () => {
    expect(describeError({ code: "weird", message: "x", kind: null }, "en")).toBe("Something went wrong: x");
    expect(describeError(new Error("boom"), "en")).toBe("Something went wrong: boom");
    expect(describeError("plain", "tr")).toBe("Bir şeyler ters gitti: plain");
  });
  it("recognises command errors", () => {
    expect(isCommandError({ code: "config", message: "m", kind: null })).toBe(true);
    expect(isCommandError({ message: "m" })).toBe(false);
    expect(isCommandError(null)).toBe(false);
  });
});
