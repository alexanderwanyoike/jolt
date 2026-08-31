# Schema migrations

```meta
Guide: 03
Kicker: JOLT DATA SDK GUIDE
Level: Fundamentals
SDK: jolt-sdk/data
Description: Change a Schema Class without making application code understand every historical data shape.
```

Jolt data can outlive one version of an application. A migration turns an older
stored value into the one current Schema Class before application code sees it.

You keep `Post`, not `PostV1`, `PostV2`, and `PostV3`. Historical shapes stay
inside small, deterministic migration steps.

## Declare each step

Suppose version 1 called its text field `message`. Version 2 renamed that field
to `text`. Version 3 added a required list of tags.

@include sdks/js/guide/src/fundamentals/migrations.ts as src/posts.ts

`Migrations.rename()` returns a new value. It never adds helper fields to your
application data. A normal `.to(...)` step can perform any other pure value
transformation.

## What happens on a read

When the Data SDK reads an older record, it:

1. reads the stored schema version;
2. applies every required step in order;
3. validates the result against the current Schema Class; and
4. gives application code only the current `Post` type.

The daemon and protocol do not interpret the schema. Migration belongs to the
application-facing SDK boundary.

## Test old data directly

Use the Resource's `migrate()` helper in a focused test. Pass the historical
version and a plain stored value; do not recreate old model classes.

@include sdks/js/guide/src/fundamentals/migrations.test.ts as src/posts.test.ts

This helper exists for migration tests and lower-level tooling. Ordinary reads
apply migrations automatically.

## Keep migrations boring

A migration must produce the same output every time. It must not read the
clock, call the network, inspect the current user, or mutate its input.

Every version between the stored value and the current schema needs a step. A
missing step, a thrown transformation, or a final value that does not validate
throws `SchemaMigrationError`. Catch that error by type when a tool or advanced
application needs to report the failed version range.

`Migrations.rename()` also refuses to overwrite an existing destination or
send two old fields to the same new field. Failing is safer than silently
discarding stored data.

## Continue learning

- Return to [Data SDK fundamentals](data-sdk.html).
- See migrations in the [generated API reference](../sdk/reference.html#data.Migrations).
- Build the complete [beginner Chirp app](app-development.html).
