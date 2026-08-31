import {
  App,
  Collection,
  Field,
  Read,
  Schema,
} from "jolt-sdk/data";

@Schema({ version: 1 })
export class Note {
  @Field.string()
  text!: string;

  @Field.dateTime()
  createdAt!: Date;
}

export const Notes = Collection.create(Note, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
});

export const Notebook = App.create({
  id: "notebook.example",
  name: "Notebook",
  namespace: "notebook",
  data: { notes: Notes },
});
