import { describe, expect, it } from "vitest";

import { decodeAppCompatibilityDeclaration } from "../src/index.js";

describe("decodeAppCompatibilityDeclaration", () => {
  it("decodes an App Compatibility Declaration from its wire shape", () => {
    expect(
      decodeAppCompatibilityDeclaration({
        app_api: 1,
        required_features: { "data.documents": 1 },
        optional_features: { "data.subscriptions": 2 },
      })
    ).toEqual({
      appApi: 1,
      requiredFeatures: { "data.documents": 1 },
      optionalFeatures: { "data.subscriptions": 2 },
    });
  });

  it("rejects a non-positive App API level", () => {
    expect(() =>
      decodeAppCompatibilityDeclaration({
        app_api: 0,
        required_features: {},
        optional_features: {},
      })
    ).toThrow("App Compatibility Declaration app_api must be a positive integer");
  });

  it("rejects an invalid required feature contract level", () => {
    expect(() =>
      decodeAppCompatibilityDeclaration({
        app_api: 1,
        required_features: { "data.documents": 0 },
        optional_features: {},
      })
    ).toThrow("App Compatibility Declaration required_features must contain positive integers");
  });

  it("rejects an invalid optional feature map", () => {
    expect(() =>
      decodeAppCompatibilityDeclaration({
        app_api: 1,
        required_features: {},
        optional_features: [],
      })
    ).toThrow("App Compatibility Declaration optional_features must contain positive integers");
  });
});
