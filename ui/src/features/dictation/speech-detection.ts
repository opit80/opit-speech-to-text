/** Matches RecordingConfig::default; preserve exact defaults when resetting calibration. */
export const speechDetectionDefaults = {
  speech_floor_db: -41.938200260161125,
  noise_margin_db: 6.020599913279624,
  min_speech_seconds: 0.3,
};

/** Finite dBFS floor shared with the Rust gate for digital silence. */
export function rmsToDb(level: number): number {
  return Number.isFinite(level) && level > 0.000001 ? 20 * Math.log10(level) : -120;
}

/** Compare exact saved thresholds so even a small calibration change can be undone. */
export function speechDetectionCustomized(recording: typeof speechDetectionDefaults): boolean {
  return (Object.keys(speechDetectionDefaults) as (keyof typeof speechDetectionDefaults)[])
    .some((field) => recording[field] !== speechDetectionDefaults[field]);
}
