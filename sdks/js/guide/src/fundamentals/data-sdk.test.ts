import { describe, expect, it } from "vitest";
import { Note } from "./data-sdk";
import { createFirstNote } from "./data-sdk-usage";

describe("Data SDK fundamentals guide", () => {
  it("uses a Resource through the generated app interface", async () => {
    const item = await createFirstNote();

    expect(item.value).toBeInstanceOf(Note);
    expect(item.value.text).toBe("Hello, Jolt!");
  });
});
