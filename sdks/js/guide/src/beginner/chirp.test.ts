import { describe, expect, it } from "vitest";
import { State } from "jolt-sdk/data";

import { Chirp, Post } from "./chirp";
import { exercisePostLifecycle } from "./posts";

describe("beginner Chirp Data SDK example", () => {
  it("uses the same typed App without a daemon or network", async () => {
    const chirp = Chirp.test({ identity: "alice.jolt" });
    const restored = await exercisePostLifecycle(
      chirp,
      new Date("2026-08-28T12:00:00.000Z"),
    );

    expect(restored.state).toBe(State.Present);
    expect(restored.value).toBeInstanceOf(Post);
    expect(restored.value).toEqual({
      text: "Hello again!",
      postedAt: new Date("2026-08-28T12:00:00.000Z"),
    });
  });
});
