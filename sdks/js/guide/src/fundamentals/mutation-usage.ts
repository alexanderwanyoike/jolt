import { Ref, State } from "jolt-sdk/data";
import { Task, TaskList } from "./mutations";

type TaskCollection = ReturnType<typeof TaskList.test>["tasks"];

export async function reviseTask(tasks: TaskCollection) {
  const created = await tasks.create({ title: "Read the guide", done: false });
  const updated = await created.update({ done: true });
  const replaced = await updated.replace({ title: "Build an app", done: true });
  const deleted = await replaced.delete();
  const restored = await deleted.restore({ title: "Build an app", done: false });

  return { created, updated, replaced, deleted, restored };
}

export async function describeTask(tasks: TaskCollection, ref: Ref<Task>) {
  const item = await tasks.get(ref);

  switch (item.state) {
    case State.Present:
      return item.value.title;
    case State.Deleted:
      return "This task was deleted";
    case State.Missing:
      return "Task not found";
    case State.Unavailable:
      return "Task is temporarily unavailable";
    default:
      throw new Error("Unknown task state");
  }
}
