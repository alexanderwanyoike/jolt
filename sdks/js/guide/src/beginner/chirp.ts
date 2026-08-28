import {
  App,
  Collection,
  Field,
  Read,
  Schema,
} from "jolt-sdk/data";

@Schema({ version: 1 })
export class Post {
  @Field.string()
  text!: string;

  @Field.dateTime()
  postedAt!: Date;
}

export const Posts = Collection.create(Post, {
  access: {
    read: Read.AnyIdentity,
    create: true,
    update: true,
    delete: true,
    restore: true,
  },
});

export const Chirp = App.create({
  id: "chirp.example",
  name: "Chirp",
  namespace: "chirp",
  data: {
    posts: Posts,
  },
});

export type ChirpApplication = ReturnType<typeof Chirp.test>;
