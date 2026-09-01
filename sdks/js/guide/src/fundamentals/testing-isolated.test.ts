import { describe, expect, it } from "vitest";
import { State } from "jolt-sdk/data";
import { TaskList } from "./mutations";

describe("one identity", () => {
  it("uses the normal typed Resource interface", async () => {
    const app = TaskList.test({ identity: "alice.jolt" });
    const created = await app.tasks.create({ title: "Write a test", done: false });
    const updated = await created.update({ done: true });

    expect(updated.value).toEqual({ title: "Write a test", done: true });
  });

  it("starts each test instance with fresh state", async () => {
    const first = TaskList.test({ identity: "alice.jolt" });
    const created = await first.tasks.create({ title: "Temporary", done: false });
    const second = TaskList.test({ identity: "alice.jolt" });

    expect((await second.tasks.get(created.ref)).state).toBe(State.Missing);
  });
});
