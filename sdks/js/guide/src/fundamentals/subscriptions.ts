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
  },
});

export const Feed = App.create({
  id: "feed.example",
  name: "Feed",
  namespace: "feed",
  data: { posts: Posts },
});
