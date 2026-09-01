import {
  App,
  Collection,
  Document,
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

@Schema({ version: 1 })
export class Following {
  @Field.array(Field.identity)
  identities!: string[];
}

@Schema({ version: 1 })
export class Profile {
  @Field.string()
  nickname!: string;
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

export const FollowingDocument = Document.create(Following, {
  access: {
    read: Read.OwnIdentity,
    create: true,
    update: true,
  },
});

export const ProfileDocument = Document.create(Profile, {
  access: {
    read: Read.AnyIdentity,
    create: true,
    update: true,
  },
});

export const Chirp = App.create({
  id: "chirp.example",
  name: "Chirp",
  namespace: "chirp",
  data: {
    posts: Posts,
    following: FollowingDocument,
    profile: ProfileDocument,
  },
});

export type ChirpApplication = Awaited<ReturnType<typeof Chirp.connect>>;
export type ChirpPost = Awaited<ReturnType<ChirpApplication["posts"]["create"]>>;
export type DeletedChirpPost = Awaited<ReturnType<ChirpPost["delete"]>>;
