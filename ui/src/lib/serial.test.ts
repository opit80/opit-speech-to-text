import { describe, expect, it } from "vitest";
import { createSerialQueue } from "./serial";

describe("createSerialQueue", () => {
  it("runs overlapping tasks in order and reads the previous task's effect", async () => {
    const enqueue = createSerialQueue();
    const state = { a: false, b: false };
    const order: string[] = [];
    let release!: () => void;
    let markStarted!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    const started = new Promise<void>((resolve) => { markStarted = resolve; });

    const first = enqueue(async () => {
      order.push("first started");
      const draft = { ...state, a: true };
      markStarted();
      await gate;
      Object.assign(state, draft);
      order.push("first finished");
      return "first result";
    });
    const second = enqueue(() => {
      order.push("second started");
      expect(state.a).toBe(true);
      Object.assign(state, { ...state, b: true });
      return "second result";
    });

    await started;
    expect(order).toEqual(["first started"]);
    expect(state).toEqual({ a: false, b: false });
    release();

    expect(await first).toBe("first result");
    expect(await second).toBe("second result");
    expect(order).toEqual(["first started", "first finished", "second started"]);
    expect(state).toEqual({ a: true, b: true });
  });

  it("preserves a caller's rejection and still runs the next queued task", async () => {
    const enqueue = createSerialQueue();
    const order: string[] = [];
    const error = new Error("save failed");
    const failed = enqueue(async () => {
      order.push("failed");
      throw error;
    });
    const next = enqueue(() => {
      order.push("next");
      return 42;
    });

    await expect(failed).rejects.toBe(error);
    expect(await next).toBe(42);
    expect(order).toEqual(["failed", "next"]);
  });
});
