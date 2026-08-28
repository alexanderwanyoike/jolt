import { describe, expect, expectTypeOf, it } from "vitest";

import {
  App,
  Collection,
  ConflictError,
  Field,
  Read,
  Schema,
  SchemaValidationError,
  State,
  type DeletedItem,
  type PresentItem,
} from "jolt-sdk/data";
import { JoltTransportError } from "jolt-sdk";
import { createFakeJolt } from "jolt-sdk/testing";

@Schema({ version: 1 })
class Note {
  @Field.string()
  text!: string;
}

const Notes = Collection.create(Note, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
});

const ArchivedNotes = Collection.create(Note, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
});

const ReadOnlyNotes = Collection.create(Note, {
  access: { read: Read.OwnIdentity },
});

const Notebook = App.create({
  id: "notebook.example",
  name: "Notebook",
  namespace: "notebook",
  data: { notes: Notes, archivedNotes: ArchivedNotes },
});

const ReadOnlyNotebook = App.create({
  id: "read-only-notebook.example",
  name: "Read-only Notebook",
  namespace: "read-only-notebook",
  data: { notes: ReadOnlyNotes },
});

const implementations = [
  {
    name: "deterministic",
    connect: async () => Notebook.test({ identity: "alice.jolt" }),
  },
  {
    name: "client-backed",
    connect: async () => {
      const jolt = createFakeJolt("alice.jolt");
      return Notebook.connect({ identity: jolt.identity, client: jolt.client });
    },
  },
] as const;

describe.each(implementations)("Data SDK $name bulk mutations", ({ connect }) => {
  it("creates every valid input and reports validation failures by input index", async () => {
    const notebook = await connect();

    const result = await notebook.notes.createMany([
      { text: "First" },
      { text: 42 as never },
      { text: "Third" },
    ]);

    expect(result.succeeded.map(entry => entry.index)).toEqual([0, 2]);
    expect(result.succeeded.map(entry => entry.item.value.text)).toEqual(["First", "Third"]);
    expect(result.failed.map(entry => entry.index)).toEqual([1]);
    expect(result.failed[0]?.error).toBeInstanceOf(SchemaValidationError);
    expectTypeOf(result.succeeded[0]!.item).toMatchTypeOf<PresentItem<Note>>();
  });

  it("reports an Item from another Collection as an indexed failure", async () => {
    const notebook = await connect();
    const archived = await notebook.archivedNotes.create({ text: "Archived" });

    const result = await notebook.notes.updateMany([
      { item: archived, patch: { text: "Wrong Collection" } },
    ]);

    expect(result.succeeded).toEqual([]);
    expect(result.failed[0]).toMatchObject({ index: 0, error: expect.any(TypeError) });
    expect((await notebook.archivedNotes.get(archived.ref)).isPresent()).toBe(true);
  });

  it("updates independent Items without rolling back a success after a stale conflict", async () => {
    const notebook = await connect();
    const [first, second] = (await notebook.notes.createMany([
      { text: "First" },
      { text: "Second" },
    ])).succeeded.map(entry => entry.item);
    if (first === undefined || second === undefined) throw new Error("expected seeded notes");
    await first.update({ text: "Already changed" });

    const result = await notebook.notes.updateMany([
      { item: second, patch: { text: "Updated" } },
      { item: first, patch: { text: "Stale" } },
    ]);

    expect(result.succeeded.map(entry => entry.index)).toEqual([0]);
    expect(result.succeeded[0]?.item.value.text).toBe("Updated");
    expect(result.failed.map(entry => entry.index)).toEqual([1]);
    expect(result.failed[0]?.error).toBeInstanceOf(ConflictError);
    const stillUpdated = await notebook.notes.get(second.ref);
    expect(stillUpdated.isPresent() && stillUpdated.value.text).toBe("Updated");
  });

  it("deletes independent Items and retains the typed stale failure", async () => {
    const notebook = await connect();
    const [first, second] = (await notebook.notes.createMany([
      { text: "First" },
      { text: "Second" },
    ])).succeeded.map(entry => entry.item);
    if (first === undefined || second === undefined) throw new Error("expected seeded notes");
    await second.update({ text: "Already changed" });

    const result = await notebook.notes.deleteMany([first, second]);

    expect(result.succeeded.map(entry => entry.index)).toEqual([0]);
    expect(result.succeeded[0]?.item.state).toBe(State.Deleted);
    expect(result.failed.map(entry => entry.index)).toEqual([1]);
    expect(result.failed[0]?.error).toBeInstanceOf(ConflictError);
    expectTypeOf(result.succeeded[0]!.item).toMatchTypeOf<DeletedItem<Note>>();
  });

  it("restores independent Items and reports a stale Tombstone without rollback", async () => {
    const notebook = await connect();
    const [first, second] = (await notebook.notes.createMany([
      { text: "First" },
      { text: "Second" },
    ])).succeeded.map(entry => entry.item);
    if (first === undefined || second === undefined) throw new Error("expected seeded notes");
    const firstDeleted = await first.delete();
    const secondDeleted = await second.delete();
    await secondDeleted.restore({ text: "Already restored" });

    const result = await notebook.notes.restoreMany([
      { item: firstDeleted, value: { text: "Restored" } },
      { item: secondDeleted, value: { text: "Stale restore" } },
    ]);

    expect(result.succeeded.map(entry => entry.index)).toEqual([0]);
    expect(result.succeeded[0]?.item.value.text).toBe("Restored");
    expect(result.failed.map(entry => entry.index)).toEqual([1]);
    expect(result.failed[0]?.error).toBeInstanceOf(ConflictError);
  });
});

it("exposes bulk mutations only for the corresponding declared access", () => {
  const writable = Notebook.test().notes;
  const readOnly = ReadOnlyNotebook.test().notes;

  expectTypeOf(writable).toHaveProperty("createMany");
  expectTypeOf(writable).toHaveProperty("updateMany");
  expectTypeOf(writable).toHaveProperty("deleteMany");
  expectTypeOf(writable).toHaveProperty("restoreMany");
  expectTypeOf(readOnly).not.toHaveProperty("createMany");
  expectTypeOf(readOnly).not.toHaveProperty("updateMany");
  expectTypeOf(readOnly).not.toHaveProperty("deleteMany");
  expectTypeOf(readOnly).not.toHaveProperty("restoreMany");
  expect(writable).toMatchObject({
    createMany: expect.any(Function),
    updateMany: expect.any(Function),
    deleteMany: expect.any(Function),
    restoreMany: expect.any(Function),
  });
  expect(readOnly).not.toHaveProperty("createMany");
});

it("retries a lost client response without changing an item mutation ID", async () => {
  const jolt = createFakeJolt("alice.jolt");
  const lostResponses = new Set(["create", "update", "delete", "restore"]);
  const mutationIds: Record<string, string[]> = {
    update: [],
    delete: [],
    restore: [],
  };
  const loseOnce = <T>(operation: string, result: T): T => {
    if (lostResponses.delete(operation)) {
      throw new JoltTransportError(`Lost ${operation} response`);
    }
    return result;
  };
  const client = {
    ...jolt.client,
    async publishJson(...args: Parameters<typeof jolt.client.publishJson>) {
      return loseOnce("create", await jolt.client.publishJson(...args));
    },
    async updateRecord(...args: Parameters<typeof jolt.client.updateRecord>) {
      mutationIds.update.push(args[2].mutationId);
      return loseOnce("update", await jolt.client.updateRecord(...args));
    },
    async deleteRecord(...args: Parameters<typeof jolt.client.deleteRecord>) {
      mutationIds.delete.push(args[1].mutationId);
      return loseOnce("delete", await jolt.client.deleteRecord(...args));
    },
    async restoreRecord(...args: Parameters<typeof jolt.client.restoreRecord>) {
      mutationIds.restore.push(args[2].mutationId);
      return loseOnce("restore", await jolt.client.restoreRecord(...args));
    },
  };
  const notebook = await Notebook.connect({ identity: jolt.identity, client });

  const created = (await notebook.notes.createMany([{ text: "Created" }])).succeeded[0]!.item;
  const updated = (await notebook.notes.updateMany([
    { item: created, patch: { text: "Updated" } },
  ])).succeeded[0]!.item;
  const deleted = (await notebook.notes.deleteMany([updated])).succeeded[0]!.item;
  const restored = (await notebook.notes.restoreMany([
    { item: deleted, value: { text: "Restored" } },
  ])).succeeded[0]!.item;

  expect(restored.value.text).toBe("Restored");
  expect(mutationIds.update[0]).toBe(mutationIds.update[1]);
  expect(mutationIds.delete[0]).toBe(mutationIds.delete[1]);
  expect(mutationIds.restore[0]).toBe(mutationIds.restore[1]);
});
