export type ConsoleRouteId =
  | "overview"
  | "identity"
  | "apps"
  | "network"
  | "relays"
  | "published"
  | "cache"
  | "settings"
  | "diagnostics";

export type ConsoleRoute = {
  id: ConsoleRouteId;
  label: string;
  path: string;
};

export const consoleRoutes: ConsoleRoute[] = [
  { id: "overview", label: "Overview", path: "/" },
  { id: "identity", label: "Identity", path: "/identity" },
  { id: "apps", label: "Apps", path: "/apps" },
  { id: "network", label: "Network", path: "/network" },
  { id: "relays", label: "Relays", path: "/relays" },
  { id: "published", label: "Published", path: "/published" },
  { id: "cache", label: "Cache", path: "/cache" },
  { id: "settings", label: "Settings", path: "/settings" },
  { id: "diagnostics", label: "Diagnostics", path: "/diagnostics" }
];
