import { describe, expect, it } from "vitest";
import { nextDemoStep } from "./guide-demo";

describe("guide demo", () => {
  it("walks from cursor placement to the pasted result and starts over", () => {
    expect(nextDemoStep("cursor")).toBe("speak");
    expect(nextDemoStep("speak")).toBe("process");
    expect(nextDemoStep("process")).toBe("result");
    expect(nextDemoStep("result")).toBe("cursor");
  });
});
