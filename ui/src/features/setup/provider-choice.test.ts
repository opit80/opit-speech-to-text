import { expect, it } from "vitest";
import { restoreSavedGroqChoice } from "./provider-choice";

function pendingLookup() {
  let resolve!: (has: boolean) => void;
  const result = new Promise<boolean>((done) => { resolve = done; });
  return { result, resolve };
}

it("restores a saved Groq choice on a Back visit", async () => {
  let choice = "elevenlabs";
  restoreSavedGroqChoice("work-groq", async (ref) => ref === "work-groq", () => { choice = "groq"; });
  await Promise.resolve();
  expect(choice).toBe("groq");
});

it("keeps the recommendation when Groq has no stored key", async () => {
  let choice = "elevenlabs";
  restoreSavedGroqChoice("groq", async () => false, () => { choice = "groq"; });
  await Promise.resolve();
  expect(choice).toBe("elevenlabs");
});

it("ignores a late stored-key reply after interaction or disposal cancels restoration", async () => {
  const lookup = pendingLookup();
  let choice = "elevenlabs";
  const cancel = restoreSavedGroqChoice("groq", () => lookup.result, () => { choice = "groq"; });
  cancel();
  lookup.resolve(true);
  await lookup.result;
  expect(choice).toBe("elevenlabs");
});

it("does not query credentials without a Groq key reference", async () => {
  let queried = false;
  let restored = false;
  restoreSavedGroqChoice(null, async () => { queried = true; return true; }, () => { restored = true; });
  await Promise.resolve();
  expect({ queried, restored }).toEqual({ queried: false, restored: false });
});

it("keeps the recommendation if the stored-key lookup fails", async () => {
  let choice = "elevenlabs";
  restoreSavedGroqChoice("groq", async () => { throw new Error("unavailable"); }, () => { choice = "groq"; });
  await Promise.resolve();
  await Promise.resolve();
  expect(choice).toBe("elevenlabs");
});
