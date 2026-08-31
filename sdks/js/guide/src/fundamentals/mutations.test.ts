import { describe, expect, it } from "vitest";
import {
  AccessRevokedError,
  ConflictError,
  ItemUnavailableError,
  SchemaValidationError,
  State,
} from "jolt-sdk/data";
import { mutationMessage } from "./mutation-errors";
import { describeTask, reviseTask } from "./mutation-usage";
import { TaskList } from "./mutations";

describe("Data SDK mutations guide", () => {
  it("returns a new immutable Item for every lifecycle step", async () => {
    const taskList = TaskList.test({ identity: "alice.jolt" });
    const result = await reviseTask(taskList.tasks);

    expect(result.created.value).toEqual({ title: "Read the guide", done: false });
    expect(result.updated.value).toEqual({ title: "Read the guide", done: true });
    expect(result.replaced.value).toEqual({ title: "Build an app", done: true });
    expect(result.deleted.state).toBe(State.Deleted);
    expect(result.restored.value).toEqual({ title: "Build an app", done: false });
    expect(result.restored.ref).toEqual(result.created.ref);
    expect(await describeTask(taskList.tasks, result.created.ref)).toBe("Build an app");

    const removed = await taskList.tasks.create({ title: "Remove me", done: false });
    await removed.delete();
    expect(await describeTask(taskList.tasks, removed.ref)).toBe("This task was deleted");
    expect(await describeTask(taskList.tasks, {
      identity: "alice.jolt",
      path: "/tasks/tasks/missing",
    })).toBe("Task not found");
  });

  it("reports expected failures by error type", () => {
    const ref = { identity: "alice.jolt", path: "/tasks/tasks/one" } as const;

    expect(mutationMessage(new ConflictError(ref))).toMatch(/changed/);
    expect(mutationMessage(new ItemUnavailableError(ref))).toMatch(/right now/);
    expect(mutationMessage(new AccessRevokedError())).toMatch(/approval/);
    expect(mutationMessage(new SchemaValidationError("title", "must be a string")))
      .toBe("Check the title field.");
    const unexpected = new Error("unexpected");
    expect(() => mutationMessage(unexpected)).toThrow(unexpected);
  });
});
