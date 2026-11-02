/** Restore a configured Groq account without making Groq the fresh-install recommendation. */
export function restoreSavedGroqChoice(
  ref: string | null,
  hasApiKey: (ref: string) => Promise<boolean>,
  restore: () => void,
): () => void {
  let cancelled = false;
  if (ref) void hasApiKey(ref).then((has) => { if (has && !cancelled) restore(); }).catch(() => {});
  return () => { cancelled = true; };
}
