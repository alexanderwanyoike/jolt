import { describe, expect, it } from "vitest";
import {
  isJoltUnavailableError,
  JoltApiError,
  JoltTransportError,
} from "../src/errors.js";

describe("isJoltUnavailableError", () => {
  it("classifies a typed transport failure as unavailable", () => {
    expect(
      isJoltUnavailableError(
        new JoltTransportError("Cannot reach the Jolt daemon"),
      ),
    ).toBe(true);
  });

  it.each([500, 502])(
    "classifies host gateway status %i as unavailable",
    (status) => {
      expect(
        isJoltUnavailableError(new JoltApiError("Host gateway failed", { status })),
      ).toBe(true);
    },
  );

  it.each([
    new JoltApiError("Unauthorized", { status: 401 }),
    new JoltApiError("Feature discovery not found", { status: 404 }),
    new TypeError("Application decoder bug"),
    new Error("Unexpected application failure"),
  ])("does not weaken a non-availability failure", (error) => {
    expect(isJoltUnavailableError(error)).toBe(false);
  });
});
