# Change an Item

```meta
Guide: 04
Kicker: JOLT DATA SDK GUIDE
Level: Fundamentals
SDK: jolt-sdk/data
Description: Update, replace, delete, and restore typed Items without handling Jolt's storage machinery.
```

A Resource decides which changes an application may make. TypeScript then
exposes only those methods on its Items. Application code does not manage paths,
content identifiers, or concurrency bookkeeping.

## Declare the allowed changes

This Task Collection allows its owner to perform the complete Item lifecycle:

@include sdks/js/guide/src/fundamentals/mutations.ts as src/tasks.ts

Remove an operation from `access` when the application does not need it. For
example, without `delete: true`, a Present Item has no `delete()` method.

## Keep the new Item

Items are immutable snapshots. A successful mutation returns a new Item at the
same stable `ref`; it does not change the older snapshot in place.

@include sdks/js/guide/src/fundamentals/mutation-usage.ts as src/task-actions.ts

`update()` is a shallow patch. Omitted fields stay unchanged, while a supplied
array or nested value replaces that whole field. Use `replace()` when you mean
to supply the complete value.

`delete()` returns a Deleted Item. `restore()` also takes a complete value,
which must match the current Schema Class. This lets an application restore old
logical data into its current shape.

## Read the Item state

The `describeTask()` example uses a normal `switch` over `item.state`:

- `State.Present` has a schema-valid `value`.
- `State.Deleted` is deliberately deleted and may be restorable.
- `State.Missing` has no known record at that logical reference.
- `State.Unavailable` means Jolt cannot safely determine the current state.

The matching `isPresent()` and `isDeleted()` helpers are useful when an early
return reads more clearly than a switch. Both forms narrow the TypeScript type.

## Catch expected failures by type

Keep error handling close to the user action. Catch the specific failures the
screen can explain, and rethrow anything it does not understand:

@include sdks/js/guide/src/fundamentals/mutation-errors.ts as src/mutation-errors.ts

`ConflictError` means the Item snapshot became stale before the mutation. Read
the current Item, show it to the user when needed, and retry only if the action
still makes sense. `ItemUnavailableError` prevents a change when current state
is unknown. `AccessRevokedError` asks the application to reconnect and request
approval again. `SchemaValidationError` identifies invalid application data.

## Automatic conflicts are the default

Resources need no conflict configuration for normal application code. Jolt
combines concurrent changes to different top-level fields, resolves concurrent
changes to the same field deterministically, and lets deletion win over a
concurrent update.

That automatic distributed behavior is separate from a `ConflictError` raised
when application code tries to mutate an already stale Item snapshot. Advanced
applications can override the automatic policies and expose Manual alternatives;
the beginner path does not need that machinery.

## Continue learning

- Return to [Data SDK fundamentals](data-sdk.html).
- Evolve stored values with [Schema migrations](data-sdk-migrations.html).
- Build these actions into the [beginner Chirp app](app-development.html).
