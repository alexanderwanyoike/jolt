import { describe, expect, it } from "vitest";

import {
  Field,
  Migrations,
  Schema,
  SchemaMigrationError,
  SchemaValidationError,
} from "jolt-sdk/data";
import type { Identity } from "jolt-sdk/data";

describe("Schema classes", () => {
  it("validates a typed value and converts date-time fields", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;

      @Field.dateTime()
      postedAt!: Date;
    }

    const post: Post = Schema.parse(Post, {
      text: "Hello!",
      postedAt: "2026-08-26T10:00:00.000Z",
    });

    expect(post.text).toBe("Hello!");
    expect(post.postedAt).toEqual(new Date("2026-08-26T10:00:00.000Z"));
  });

  it("rejects implementation-defined non-ISO date-time strings", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.dateTime()
      postedAt!: Date;
    }

    expect(() => Schema.parse(Post, { postedAt: "1" })).toThrow(SchemaValidationError);
    expect(() => Schema.parse(Post, { postedAt: "2026/08/26" })).toThrow(SchemaValidationError);
  });

  it("throws a typed validation error for an invalid field", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    expect(() => Schema.parse(Post, { text: 42 })).toThrow(SchemaValidationError);
  });

  it("throws a typed validation error for a non-object schema value", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    expect(() => Schema.parse(Post, null)).toThrow(SchemaValidationError);
  });

  it("explains that Schema Classes require legacy TypeScript decorators", () => {
    const standardDecoratorCall = Field.string() as unknown as (
      value: undefined,
      context: { readonly kind: "field"; readonly name: string },
    ) => void;

    expect(() => standardDecoratorCall(undefined, {
      kind: "field",
      name: "text",
    })).toThrow(/experimentalDecorators/);
  });

  it("rejects invalid Schema Class versions at definition time", () => {
    expect(() => Schema({ version: 0 })).toThrow(/positive integer/);
    expect(() => Schema({ version: 1.5 })).toThrow(/positive integer/);
  });

  it("rejects invalid migration target versions at definition time", () => {
    const migrations = Migrations.create();

    expect(() => migrations.to(0, value => value)).toThrow(/positive integer/);
    expect(() => migrations.to(1.5, value => value)).toThrow(/positive integer/);
  });

  it("rejects duplicate migration target versions", () => {
    const migrations = Migrations.create()
      .to(2, value => value);

    expect(() => migrations.to(2, value => value)).toThrow(/already defined/);
  });

  it("rejects invalid stored schema versions before migration", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    expect(() => Schema.migrate(Post, {
      version: 0,
      value: { text: "Hello!" },
    })).toThrow(/positive integer/);
  });

  it("validates number, boolean, and identity fields", () => {
    @Schema({ version: 1 })
    class Settings {
      @Field.number()
      refreshMinutes!: number;

      @Field.boolean()
      notifications!: boolean;

      @Field.identity()
      owner!: Identity;
    }

    expect(Schema.parse(Settings, {
      refreshMinutes: 15,
      notifications: true,
      owner: "alice.jolt",
    })).toMatchObject({
      refreshMinutes: 15,
      notifications: true,
      owner: "alice.jolt",
    });
  });

  it("leaves omitted optional fields absent", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;

      @Field.string({ optional: true })
      summary?: string;
    }

    const post = Schema.parse(Post, { text: "Hello!" });

    expect(post.summary).toBeUndefined();
    expect("summary" in post).toBe(false);
  });

  it("validates nested schema fields", () => {
    @Schema({ version: 1 })
    class Profile {
      @Field.string()
      displayName!: string;
    }

    @Schema({ version: 1 })
    class Post {
      @Field.schema(Profile)
      author!: Profile;
    }

    const post = Schema.parse(Post, {
      author: { displayName: "Alice" },
    });

    expect(post.author).toBeInstanceOf(Profile);
    expect(post.author.displayName).toBe("Alice");
  });

  it("validates arrays of primitive and nested schema values", () => {
    @Schema({ version: 1 })
    class Attachment {
      @Field.string()
      url!: string;
    }

    @Schema({ version: 1 })
    class Post {
      @Field.array(Field.string)
      tags!: string[];

      @Field.array(Field.schema(Attachment))
      attachments!: Attachment[];
    }

    const post = Schema.parse(Post, {
      tags: ["jolt", "hello"],
      attachments: [{ url: "https://example.test/photo.jpg" }],
    });

    expect(post.tags).toEqual(["jolt", "hello"]);
    expect(post.attachments[0]).toBeInstanceOf(Attachment);
    expect(post.attachments[0]?.url).toBe("https://example.test/photo.jpg");
  });

  it("rejects optional array item descriptors", () => {
    expect(() => Field.array(Field.string({ optional: true }))).toThrow(
      /array field instead of its items/,
    );
  });

  it("migrates an older value into the current schema without an old model class", () => {
    const migrations = Migrations.create()
      .to(2, migration => migration.rename("message", "text"));

    @Schema({ version: 2, migrations })
    class Post {
      @Field.string()
      text!: string;
    }

    const post = Schema.migrate(Post, {
      version: 1,
      value: { message: "Hello!" },
    });

    expect(post).toBeInstanceOf(Post);
    expect(post.text).toBe("Hello!");
  });

  it("supports pure migration transforms", () => {
    const migrations = Migrations.create()
      .to(2, migration => migration.rename("message", "text"))
      .to(3, value => ({
        ...value,
        tags: ["migrated"],
      }));

    @Schema({ version: 3, migrations })
    class Post {
      @Field.string()
      text!: string;

      @Field.array(Field.string)
      tags!: string[];
    }

    const post = Schema.migrate(Post, {
      version: 1,
      value: { message: "Hello!" },
    });

    expect(post.text).toBe("Hello!");
    expect(post.tags).toEqual(["migrated"]);
  });

  it("supports optional arrays and optional nested schemas without treating null as absent", () => {
    @Schema({ version: 1 })
    class Metadata {
      @Field.string()
      source!: string;
    }

    @Schema({ version: 1 })
    class Post {
      @Field.array(Field.string, { optional: true })
      tags?: string[];

      @Field.schema(Metadata, { optional: true })
      metadata?: Metadata;
    }

    const post = Schema.parse(Post, {});
    expect(post.tags).toBeUndefined();
    expect(post.metadata).toBeUndefined();

    expect(() => Schema.parse(Post, { tags: null })).toThrow(SchemaValidationError);
    expect(() => Schema.parse(Post, { metadata: null })).toThrow(SchemaValidationError);
  });

  it("throws a typed error when a required migration step is missing", () => {
    @Schema({ version: 2 })
    class Post {
      @Field.string()
      text!: string;
    }

    expect(() => Schema.migrate(Post, {
      version: 1,
      value: { text: "Hello!" },
    })).toThrow(SchemaMigrationError);
  });

  it("keeps current-version validation failures distinct from migration failures", () => {
    @Schema({ version: 1 })
    class Post {
      @Field.string()
      text!: string;
    }

    expect(() => Schema.migrate(Post, {
      version: 1,
      value: { text: 42 },
    })).toThrow(SchemaValidationError);
  });
});
