import { describe, expect, it } from "vitest";
import { parseRoute, routeHash } from "./route";

describe("parseRoute", () => {
  it("reads the route from the hash", () => {
    expect(parseRoute("#/settings")).toBe("settings");
    expect(parseRoute("#settings")).toBe("settings");
    expect(parseRoute("#/history?q=x")).toBe("history");
    expect(parseRoute("#/rules/extra")).toBe("rules");
    expect(parseRoute("#/guide")).toBe("guide");
    expect(parseRoute("#/usage")).toBe("usage");
  });
  it("falls back to home", () => {
    expect(parseRoute("")).toBe("home");
    expect(parseRoute("#/")).toBe("home");
    expect(parseRoute("#/nope")).toBe("home");
  });
  it("round-trips", () => expect(parseRoute(routeHash("setup"))).toBe("setup"));
});
