/** The guide is a local illustration, independent of the dictation controller. */
export const DEMO_STEPS = ["cursor", "speak", "process", "result"] as const;
export type DemoStep = (typeof DEMO_STEPS)[number];

export function nextDemoStep(step: DemoStep): DemoStep {
  return DEMO_STEPS[(DEMO_STEPS.indexOf(step) + 1) % DEMO_STEPS.length];
}
