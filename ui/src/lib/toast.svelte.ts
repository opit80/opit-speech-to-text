// Short-lived confirmations ("Saved", "Copied") and non-blocking errors.
export interface Toast {
  id: number;
  text: string;
  tone: "info" | "error";
}

export const toasts = $state<Toast[]>([]);
let next = 1;

export function toast(text: string, tone: Toast["tone"] = "info"): void {
  const id = next++;
  toasts.push({ id, text, tone });
  setTimeout(() => {
    const index = toasts.findIndex((t) => t.id === id);
    if (index >= 0) toasts.splice(index, 1);
  }, tone === "error" ? 6000 : 2500);
}
