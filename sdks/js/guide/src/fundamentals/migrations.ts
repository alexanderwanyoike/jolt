import {
  Collection,
  Field,
  Migrations,
  Read,
  Schema,
  SchemaMigrationError,
} from "jolt-sdk/data";

export const PostMigrations = Migrations.create()
  .to(2, value => Migrations.rename(value, { message: "text" }))
  .to(3, value => ({
    ...value,
    tags: value.tags ?? [],
  }));

@Schema({ version: 3, migrations: PostMigrations })
export class Post {
  @Field.string()
  text!: string;

  @Field.array(Field.string)
  tags!: string[];

  @Field.dateTime()
  postedAt!: Date;
}

export const Posts = Collection.create(Post, {
  access: {
    read: Read.AnyIdentity,
    create: true,
    update: true,
  },
});

export function explainMigrationFailure(error: unknown): string {
  if (error instanceof SchemaMigrationError) {
    return `Could not upgrade schema ${error.fromVersion} to ${error.toVersion}`;
  }
  throw error;
}
