import {
  App,
  Collection,
  Field,
  Read,
  Schema,
} from "jolt-sdk/data";

@Schema({ version: 1 })
export class Task {
  @Field.string()
  title!: string;

  @Field.boolean()
  done!: boolean;
}

export const Tasks = Collection.create(Task, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
});

export const TaskList = App.create({
  id: "task-list.example",
  name: "Task List",
  namespace: "tasks",
  data: { tasks: Tasks },
});
