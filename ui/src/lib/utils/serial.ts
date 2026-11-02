/** Runs tasks one at a time, preserving each result without letting failures block the queue. */
export function createSerialQueue() {
  let chain: Promise<unknown> = Promise.resolve();
  return function enqueue<T>(task: () => T | PromiseLike<T>): Promise<T> {
    const run = chain.catch(() => {}).then(task);
    chain = run;
    return run;
  };
}
