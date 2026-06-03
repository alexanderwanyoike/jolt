import { describe, expect, it } from "vitest";
import { consoleRoutes } from "./navigation";

describe("consoleRoutes", () => {
  it("keeps the v0 console sections addressable", () => {
    expect(consoleRoutes.map((route) => route.label)).toEqual([
      "Overview",
      "Identity",
      "Apps",
      "Network",
      "Relays",
      "Published",
      "Cache",
      "Settings",
      "Diagnostics"
    ]);

    expect(new Set(consoleRoutes.map((route) => route.path)).size).toBe(consoleRoutes.length);
  });
});
