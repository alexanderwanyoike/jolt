import { describe, expect, expectTypeOf, it } from "vitest";

import {
  App,
  Collection,
  ConflictError,
  DeleteConflict,
  Field,
  Read,
  Schema,
  SchemaValidationError,
  State,
  UpdateConflict,
} from "jolt-sdk/data";

@Schema({ version: 1 })
class Note {
  @Field.string()
  text!: string;
}

const Notes = Collection.create(Note, {
  access: {
    read: Read.AnyIdentity,
    create: true,
    update: true,
  },
  conflicts: {
    update: UpdateConflict.Manual,
    delete: DeleteConflict.DeleteWins,
  },
});

const Notebook = App.create({
  id: "notebook.example",
  name: "Notebook",
  namespace: "notebook",
  data: { notes: Notes },
});

const AutomaticNotes = Collection.create(Note, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.DeleteWins,
  },
});

const AutomaticNotebook = App.create({
  id: "automatic-notebook.example",
  name: "Automatic Notebook",
  namespace: "automatic-notebook",
  data: { notes: AutomaticNotes },
});

@Schema({ version: 1 })
class CollaborativeNote {
  @Field.string()
  text!: string;

  @Field.boolean()
  pinned!: boolean;
}

const CollaborativeNotes = Collection.create(CollaborativeNote, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.DeleteWins,
  },
});

const CollaborativeNotebook = App.create({
  id: "collaborative-notebook.example",
  name: "Collaborative Notebook",
  namespace: "collaborative-notebook",
  data: { notes: CollaborativeNotes },
});

describe("Data SDK Manual conflicts", () => {
  it("exposes concurrent alternatives and resolves by choosing one", async () => {
    const world = Notebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }

    await phoneCopy.update({ text: "Phone edit" });
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const conflicted = await phone.notes.get(created.ref);
    expect(conflicted.state).toBe(State.Conflicted);
    expect(conflicted.isConflicted()).toBe(true);
    if (!conflicted.isConflicted()) throw new Error("expected a Manual conflict");

    expect(conflicted.alternatives.map(alternative => (
      alternative.isPresent() ? alternative.value.text : "deleted"
    ))).toEqual(["Laptop edit", "Phone edit"]);
    expect(Object.isFrozen(conflicted.alternatives)).toBe(true);
    expect(conflicted.alternatives.every(Object.isFrozen)).toBe(true);
    expect(conflicted.alternatives.every(alternative => (
      !("update" in alternative) && !("delete" in alternative)
    ))).toBe(true);

    const phoneEdit = conflicted.alternatives.find(alternative => (
      alternative.isPresent() && alternative.value.text === "Phone edit"
    ));
    if (phoneEdit === undefined || !phoneEdit.isPresent()) {
      throw new Error("expected the present phone alternative");
    }
    const resolved = await conflicted.choose(phoneEdit);

    expect(resolved.isPresent()).toBe(true);
    if (!resolved.isPresent()) throw new Error("expected a present resolution");
    expect(resolved.value.text).toBe("Phone edit");

    await world.sync();
    const converged = await laptop.notes.get(created.ref);
    expect(converged.isPresent()).toBe(true);
    expect(converged.isConflicted()).toBe(false);
    if (!converged.isPresent()) throw new Error("expected a present note");
    expect(converged.value.text).toBe("Phone edit");
  });

  it("resolves concurrent alternatives with a custom schema-valid value", async () => {
    const world = Notebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.update({ text: "Phone edit" });
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const conflicted = await laptop.notes.get(created.ref);
    if (!conflicted.isConflicted()) throw new Error("expected a Manual conflict");
    await expect(conflicted.resolve({ text: 42 as never })).rejects.toBeInstanceOf(
      SchemaValidationError,
    );
    const resolved = await conflicted.resolve({ text: "Combined edit" });
    expect(resolved.isPresent()).toBe(true);
    if (!resolved.isPresent()) throw new Error("expected a present resolution");
    expect(resolved.value.text).toBe("Combined edit");

    await world.sync();
    for (const device of [phone, laptop]) {
      const converged = await device.notes.get(created.ref);
      expect(converged.isPresent()).toBe(true);
      expect(converged.isConflicted()).toBe(false);
      if (!converged.isPresent()) throw new Error("expected a present note");
      expect(converged.value.text).toBe("Combined edit");
    }
  });

  it("does not add Manual conflict handling to an automatic-policy Resource", async () => {
    const notebook = AutomaticNotebook.test();
    const created = await notebook.notes.create({ text: "Simple" });

    expect("isConflicted" in created).toBe(false);
    expectTypeOf(created).not.toHaveProperty("isConflicted");
  });

  it("rejects a resolution from a stale Conflict Item", async () => {
    const world = Notebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.update({ text: "Phone edit" });
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const first = await phone.notes.get(created.ref);
    const stale = await phone.notes.get(created.ref);
    if (!first.isConflicted() || !stale.isConflicted()) {
      throw new Error("expected two Manual conflict snapshots");
    }
    await first.resolve({ text: "Resolved" });
    await expect(stale.resolve({ text: "Stale resolution" })).rejects.toBeInstanceOf(
      ConflictError,
    );
  });

  it("shares one synchronized history between named devices and default views", async () => {
    const world = Notebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const bob = world.as("bob.jolt");
    const created = await phone.notes.create({ text: "From Alice's phone" });

    await world.sync();
    const observed = await bob.notes.for("alice.jolt").get(created.ref);

    expect(observed.isPresent()).toBe(true);
    if (!observed.isPresent()) throw new Error("expected Alice's synchronized note");
    expect(observed.value.text).toBe("From Alice's phone");
  });

  it("orders alternatives by locale-independent revision text", async () => {
    const world = Notebook.testWorld();
    const zDevice = world.device("alice.jolt", "z-device");
    const umlautDevice = world.device("alice.jolt", "ä-device");
    const created = await zDevice.notes.create({ text: "Original" });

    await world.sync();
    const zCopy = await zDevice.notes.get(created.ref);
    const umlautCopy = await umlautDevice.notes.get(created.ref);
    if (!zCopy.isPresent() || !umlautCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await zCopy.update({ text: "Z edit" });
    await umlautCopy.update({ text: "Umlaut edit" });
    await world.sync();

    const conflicted = await zDevice.notes.get(created.ref);
    if (!conflicted.isConflicted()) throw new Error("expected a Manual conflict");
    expect(conflicted.alternatives.map(alternative => (
      alternative.isPresent() ? alternative.value.text : "deleted"
    ))).toEqual(["Z edit", "Umlaut edit"]);
  });

  it("combines concurrent updates to different schema fields", async () => {
    const world = CollaborativeNotebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original", pinned: false });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.update({ text: "Edited on phone" });
    await laptopCopy.update({ pinned: true });
    await world.sync();

    const merged = await phone.notes.get(created.ref);

    expect(merged.isPresent()).toBe(true);
    if (!merged.isPresent()) throw new Error("expected an automatic merged note");
    expect(merged.value).toEqual({ text: "Edited on phone", pinned: true });

    await merged.update({ text: "Edited after merge" });
    await world.sync();
    const converged = await laptop.notes.get(created.ref);
    expect(converged.isPresent()).toBe(true);
    if (!converged.isPresent()) throw new Error("expected the later update to converge");
    expect(converged.value).toEqual({ text: "Edited after merge", pinned: true });
  });

  it("uses deterministic revision order for concurrent same-field updates", async () => {
    const world = AutomaticNotebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.update({ text: "Phone edit" });
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const winner = await laptop.notes.get(created.ref);

    expect(winner.isPresent()).toBe(true);
    if (!winner.isPresent()) throw new Error("expected an automatic winner");
    expect(winner.value.text).toBe("Phone edit");
  });
});
