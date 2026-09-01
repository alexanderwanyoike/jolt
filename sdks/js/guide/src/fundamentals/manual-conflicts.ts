import {
  App,
  Collection,
  Field,
  Read,
  Schema,
  UpdateConflict,
} from "jolt-sdk/data";

@Schema({ version: 1 })
export class Note {
  @Field.string()
  text!: string;

  @Field.boolean()
  pinned!: boolean;
}

export const Notes = Collection.create(Note, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
  },
  conflicts: {
    update: UpdateConflict.Manual,
  },
});

export const Notebook = App.create({
  id: "manual-notebook.example",
  name: "Manual Notebook",
  namespace: "manual-notebook",
  data: { notes: Notes },
});
