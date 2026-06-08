import { describe, expect, it } from "vitest";
import { formatBytes, formatDuration, shortId, value } from "./format";

describe("format utilities", () => {
  it("formats absent values for dense status panels", () => {
    expect(value(undefined)).toBe("--");
    expect(value(0)).toBe("0");
  });

  it("formats daemon durations and byte counts", () => {
    expect(formatDuration(65)).toBe("1m 5s");
    expect(formatDuration(3660)).toBe("1h 1m");
    expect(formatBytes(42)).toBe("42 B");
    expect(formatBytes(4096)).toBe("4.00 KB");
  });

  it("shortens long content identifiers without losing the suffix", () => {
    expect(shortId("bafkexamplecid000000000000000001")).toBe("bafkexamplec...000001");
  });
});
