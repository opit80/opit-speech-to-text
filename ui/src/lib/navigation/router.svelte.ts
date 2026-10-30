import { parseRoute, routeHash, type Route } from "./route";

export const router = $state<{ route: Route }>({ route: parseRoute(location.hash) });

window.addEventListener("hashchange", () => {
  router.route = parseRoute(location.hash);
});

export function navigate(route: Route): void {
  router.route = route;
  if (location.hash !== routeHash(route)) location.hash = routeHash(route);
}
