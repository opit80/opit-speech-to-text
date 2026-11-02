/** Binding success alone does not prove a shortcut works with the current keyboard layout. */
export function shortcutTestAllowsNext(platform: string | undefined, keys: string, verifiedKeys: string, confirmed: boolean): boolean {
  return platform !== "linux" || (confirmed && keys === verifiedKeys);
}
