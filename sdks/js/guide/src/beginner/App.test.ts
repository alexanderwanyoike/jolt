import { describe, expect, it } from "vitest";
import { AppIncompatibleError } from "jolt-sdk/data";

import { describeStartupFailure } from "./App";

describe("beginner Chirp startup", () => {
  it("turns an incompatible Jolt error into useful beginner guidance", () => {
    const failure = describeStartupFailure(new AppIncompatibleError({} as never));

    expect(failure).toEqual({
      title: "Chirp needs a newer Jolt",
      message:
        "Update Jolt Console, then choose Check again. Chirp stopped before requesting access or changing data.",
    });
  });
});
