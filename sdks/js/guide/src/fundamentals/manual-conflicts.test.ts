import { describe, expect, it } from "vitest";
import { State } from "jolt-sdk/data";
import { resolveConcurrentEdits } from "./manual-conflict-usage";
import { Notebook } from "./manual-conflicts";

describe("Data SDK Manual conflicts guide", () => {
  it("chooses one exact concurrent alternative", async () => {
    const resolved = await resolveConcurrentEdits("choose-workstation");

    expect(resolved.state).toBe(State.Present);
    expect(resolved.value.text).toBe("Workstation edit");
  });

  it("publishes a custom schema-valid resolution", async () => {
    const resolved = await resolveConcurrentEdits("combine");

    expect(resolved.state).toBe(State.Present);
    expect(resolved.value.text).toBe("Combined edit");
  });

  it("still combines independent fields automatically", async () => {
    const world = Notebook.testWorld();
    const workstation = world.device("alice.jolt", "workstation");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await workstation.notes.create({ text: "Original", pinned: false });

    await world.sync();
    const workstationCopy = await workstation.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!workstationCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("Expected both devices to have the note");
    }

    await workstationCopy.update({ text: "Edited" });
    await laptopCopy.update({ pinned: true });
    await world.sync();

    const combined = await workstation.notes.get(created.ref);
    expect(combined.isPresent()).toBe(true);
    if (!combined.isPresent()) throw new Error("Expected a combined note");
    expect(combined.value).toEqual({ text: "Edited", pinned: true });
  });
});
