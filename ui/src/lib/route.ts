export const ROUTES = ["home", "history", "rules", "profiles", "settings", "setup"] as const;
export type Route = (typeof ROUTES)[number];

/** `#/settings` → "settings"; anything unknown → "home". Rust opens `index.html#/<route>`. */
export function parseRoute(hash: string): Route {
  const name = hash.replace(/^#\/?/, "").split(/[/?]/)[0];
  return (ROUTES as readonly string[]).includes(name) ? (name as Route) : "home";
}

export function routeHash(route: Route): string {
  return `#/${route}`;
}
