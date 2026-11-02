import { describe, expect, it } from "vitest";
import { rmsToDb, speechDetectionCustomized, speechDetectionDefaults } from "./speech-detection";

describe("microphone dB calibration", () => {
  it("measures amplitude on a logarithmic dBFS scale", () => {
    expect(rmsToDb(1)).toBe(0);
    expect(rmsToDb(0.01)).toBeCloseTo(-40);
    expect(rmsToDb(0.008)).toBeCloseTo(-41.9382);
    expect(rmsToDb(0.02)).toBeCloseTo(-33.9794);
  });

  it("keeps silence and invalid meter events finite", () => {
    for (const level of [0, -1, NaN, Infinity, 0.00000001]) {
      expect(rmsToDb(level)).toBe(-120);
    }
  });
});


describe("speech detection disclosure", () => {
  it("hides the reset only for the exact defaults", () => {
    expect(speechDetectionCustomized({ ...speechDetectionDefaults })).toBe(false);
  });
  it("reveals each customized threshold, including small saved changes", () => {
    for (const field of Object.keys(speechDetectionDefaults) as (keyof typeof speechDetectionDefaults)[]) {
      expect(speechDetectionCustomized({ ...speechDetectionDefaults, [field]: speechDetectionDefaults[field] + 0.000001 })).toBe(true);
    }
  });
});
