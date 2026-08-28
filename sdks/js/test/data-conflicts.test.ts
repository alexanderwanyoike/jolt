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
import { createFakeJolt } from "jolt-sdk/testing";

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

const ManualCollaborativeNotes = Collection.create(CollaborativeNote, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
  },
  conflicts: {
    update: UpdateConflict.Manual,
    delete: DeleteConflict.DeleteWins,
  },
});

const ManualCollaborativeNotebook = App.create({
  id: "manual-collaborative-notebook.example",
  name: "Manual Collaborative Notebook",
  namespace: "manual-collaborative-notebook",
  data: { notes: ManualCollaborativeNotes },
});

@Schema({ version: 1 })
class DeletionNote {
  @Field.string()
  text!: string;
}

const DeleteWinsNotes = Collection.create(DeletionNote, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.DeleteWins,
  },
});

const DeleteWinsNotebook = App.create({
  id: "delete-wins-notebook.example",
  name: "Delete Wins Notebook",
  namespace: "delete-wins-notebook",
  data: { notes: DeleteWinsNotes },
});

const UpdateWinsNotes = Collection.create(DeletionNote, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.UpdateWins,
  },
});

const UpdateWinsNotebook = App.create({
  id: "update-wins-notebook.example",
  name: "Update Wins Notebook",
  namespace: "update-wins-notebook",
  data: { notes: UpdateWinsNotes },
});

const ManualDeleteNotes = Collection.create(DeletionNote, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
  conflicts: {
    update: UpdateConflict.LastWriteWins,
    delete: DeleteConflict.Manual,
  },
});

const ManualDeleteNotebook = App.create({
  id: "manual-delete-notebook.example",
  name: "Manual Delete Notebook",
  namespace: "manual-delete-notebook",
  data: { notes: ManualDeleteNotes },
});

const UpdateWinsManualNotes = Collection.create(DeletionNote, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
  },
  conflicts: {
    update: UpdateConflict.Manual,
    delete: DeleteConflict.UpdateWins,
  },
});

const UpdateWinsManualNotebook = App.create({
  id: "update-wins-manual-notebook.example",
  name: "Update Wins Manual Notebook",
  namespace: "update-wins-manual-notebook",
  data: { notes: UpdateWinsManualNotes },
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

  it("combines daemon conflict heads through the connected Data backend", async () => {
    const jolt = createFakeJolt("alice.jolt");
    const path = "/collaborative-notebook/notes/jlt_connected";
    const mutations: Array<{
      readonly body: object;
      readonly mutation: {
        readonly revision: string;
        readonly mutationId: string;
        readonly observedRevisions?: readonly string[];
      };
    }> = [];
    const bytes = (value: object) => Array.from(new TextEncoder().encode(JSON.stringify({
      version: 1,
      value,
    })));
    const client = {
      ...jolt.client,
      async readRecord(ref: { identity: string; path: string }) {
        if (ref.path !== path) return jolt.client.readRecord(ref);
        return {
          state: "conflicted" as const,
          ref,
          alternatives: [
            {
              state: "present" as const,
              ref,
              contentId: "cid_laptop",
              revision: "revision_laptop",
              bytes: bytes({ text: "Original", pinned: true }),
            },
            {
              state: "present" as const,
              ref,
              contentId: "cid_phone",
              revision: "revision_phone",
              bytes: bytes({ text: "Phone edit", pinned: false }),
            },
          ],
          base: {
            state: "present" as const,
            ref,
            contentId: "cid_base",
            revision: "revision_base",
            bytes: bytes({ text: "Original", pinned: false }),
          },
        };
      },
      async updateRecord(
        ref: { identity: string; path: string },
        body: object,
        mutation: {
          readonly revision: string;
          readonly mutationId: string;
          readonly observedRevisions?: readonly string[];
        },
      ) {
        mutations.push({ body, mutation });
        return {
          state: "present" as const,
          ref,
          contentId: "cid_resolved",
          revision: "revision_resolved",
          bytes: bytes({ text: "After merge", pinned: true }),
        };
      },
    };
    const app = await CollaborativeNotebook.connect({ identity: jolt.identity, client });

    const merged = await app.notes.get({ identity: jolt.identity, path });

    expect(merged.isPresent()).toBe(true);
    if (!merged.isPresent()) throw new Error("expected a connected automatic merge");
    expect(merged.value).toEqual({ text: "Phone edit", pinned: true });

    const resolved = await merged.update({ text: "After merge" });

    expect(resolved.value).toEqual({ text: "After merge", pinned: true });
    expect(mutations).toEqual([{
      body: {
        version: 1,
        value: { text: "After merge", pinned: true },
      },
      mutation: {
        revision: "revision_phone",
        mutationId: expect.stringMatching(/^mut_/),
        observedRevisions: ["revision_laptop", "revision_phone"],
      },
    }]);
  });

  it("combines different-field updates even when same-field conflicts are Manual", async () => {
    const world = ManualCollaborativeNotebook.testWorld();
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

  it("falls back to deterministic whole-value order after criss-cross resolutions", async () => {
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
    await phoneCopy.update({ text: "Phone first edit" });
    await laptopCopy.update({ text: "Laptop first edit" });
    await world.sync();

    const phoneWinner = await phone.notes.get(created.ref);
    const laptopWinner = await laptop.notes.get(created.ref);
    if (!phoneWinner.isPresent() || !laptopWinner.isPresent()) {
      throw new Error("expected both devices to evaluate the first conflict");
    }
    await phoneWinner.update({ text: "Phone second edit" });
    await laptopWinner.update({ text: "Laptop second edit" });
    await world.sync();

    const resolved = await phone.notes.get(created.ref);

    expect(resolved.isPresent()).toBe(true);
    if (!resolved.isPresent()) throw new Error("expected a deterministic fallback winner");
    expect(resolved.value.text).toBe("Phone second edit");
    await resolved.update({ text: "Converged after criss-cross" });
    await world.sync();
    const converged = await laptop.notes.get(created.ref);
    expect(converged.isPresent()).toBe(true);
    if (!converged.isPresent()) throw new Error("expected the later update to converge");
    expect(converged.value.text).toBe("Converged after criss-cross");
  });

  it("applies DeleteWins to a concurrent deletion and update", async () => {
    const world = DeleteWinsNotebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.delete();
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const winner = await laptop.notes.get(created.ref);

    expect(winner.isDeleted()).toBe(true);
    if (!winner.isDeleted()) throw new Error("expected the deletion to win");
    await winner.restore({ text: "Restored after delete won" });
    await world.sync();
    const converged = await phone.notes.get(created.ref);
    expect(converged.isPresent()).toBe(true);
    if (!converged.isPresent()) throw new Error("expected the restore to converge");
    expect(converged.value.text).toBe("Restored after delete won");
  });

  it("applies UpdateWins to a concurrent deletion and update", async () => {
    const world = UpdateWinsNotebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.delete();
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const winner = await phone.notes.get(created.ref);

    expect(winner.isPresent()).toBe(true);
    if (!winner.isPresent()) throw new Error("expected the update to win");
    expect(winner.value.text).toBe("Laptop edit");
    await winner.update({ text: "Updated after update won" });
    await world.sync();
    const converged = await phone.notes.get(created.ref);
    expect(converged.isPresent()).toBe(true);
    if (!converged.isPresent()) throw new Error("expected the later update to converge");
    expect(converged.value.text).toBe("Updated after update won");
  });

  it("exposes deletion and update alternatives for DeleteConflict.Manual", async () => {
    const world = ManualDeleteNotebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
      throw new Error("expected both devices to observe the original note");
    }
    await phoneCopy.delete();
    await laptopCopy.update({ text: "Laptop edit" });
    await world.sync();

    const conflicted = await laptop.notes.get(created.ref);

    expect(conflicted.isConflicted()).toBe(true);
    if (!conflicted.isConflicted()) throw new Error("expected a Manual conflict");
    expect(conflicted.alternatives.map(alternative => alternative.state)).toEqual([
      State.Present,
      State.Deleted,
    ]);
    const deletion = conflicted.alternatives.find(alternative => alternative.isDeleted());
    if (deletion === undefined) throw new Error("expected a deleted alternative");
    await conflicted.choose(deletion);
    await world.sync();
    const converged = await phone.notes.get(created.ref);
    expect(converged.isDeleted()).toBe(true);
  });

  it("applies UpdateWins before Manual same-field resolution", async () => {
    const world = UpdateWinsManualNotebook.testWorld();
    const phone = world.device("alice.jolt", "phone");
    const laptop = world.device("alice.jolt", "laptop");
    const tablet = world.device("alice.jolt", "tablet");
    const created = await phone.notes.create({ text: "Original" });

    await world.sync();
    const phoneCopy = await phone.notes.get(created.ref);
    const laptopCopy = await laptop.notes.get(created.ref);
    const tabletCopy = await tablet.notes.get(created.ref);
    if (!phoneCopy.isPresent() || !laptopCopy.isPresent() || !tabletCopy.isPresent()) {
      throw new Error("expected every device to observe the original note");
    }
    await phoneCopy.delete();
    await laptopCopy.update({ text: "Laptop edit" });
    await tabletCopy.update({ text: "Tablet edit" });
    await world.sync();

    const conflicted = await phone.notes.get(created.ref);

    expect(conflicted.isConflicted()).toBe(true);
    if (!conflicted.isConflicted()) throw new Error("expected a Manual update conflict");
    expect(conflicted.alternatives).toHaveLength(2);
    expect(conflicted.alternatives.every(alternative => alternative.isPresent())).toBe(true);
    const selected = conflicted.alternatives[0];
    if (selected === undefined || !selected.isPresent()) {
      throw new Error("expected a present update alternative");
    }
    await conflicted.choose(selected);
    await world.sync();
    const converged = await tablet.notes.get(created.ref);
    expect(converged.isPresent()).toBe(true);
  });
});
