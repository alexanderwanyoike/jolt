import { describe, expect, it } from "vitest";
import {
  isContentUnavailableError,
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

  it.each(["content_provider_not_found", "content_fetch_failed"])(
    "does not classify reachable-daemon content code %s as daemon unavailability",
    (code) => {
      expect(
        isJoltUnavailableError(
          new JoltApiError("Record content unavailable", { status: 404, code }),
        ),
      ).toBe(false);
    },
  );

  it.each([
    new JoltApiError("Unauthorized", { status: 401 }),
    new JoltApiError("Feature discovery not found", { status: 404 }),
    new JoltApiError("Content hash mismatch", {
      status: 502,
      code: "content_hash_mismatch",
    }),
    new TypeError("Application decoder bug"),
    new Error("Unexpected application failure"),
  ])("does not weaken a non-availability failure", (error) => {
    expect(isJoltUnavailableError(error)).toBe(false);
  });
});

describe("isContentUnavailableError", () => {
  it.each(["content_provider_not_found", "content_fetch_failed"])(
    "classifies daemon content code %s as content unavailability",
    (code) => {
      expect(
        isContentUnavailableError(
          new JoltApiError("Record content unavailable", { status: 404, code }),
        ),
      ).toBe(true);
    },
  );

  it.each([
    new JoltApiError("Feature discovery not found", { status: 404 }),
    new JoltApiError("Content hash mismatch", {
      status: 502,
      code: "content_hash_mismatch",
    }),
    new JoltTransportError("Cannot reach the Jolt daemon"),
  ])("does not weaken a non-content-availability failure", (error) => {
    expect(isContentUnavailableError(error)).toBe(false);
  });
});
