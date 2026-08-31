# Resolve Manual conflicts

```meta
Guide: 05
Kicker: JOLT DATA SDK GUIDE
Level: Advanced
SDK: jolt-sdk/data
Description: Let an application choose between concurrent Item alternatives only when automatic conflict handling is not enough.
```

Most applications should keep Jolt's automatic conflict defaults. Choose a
Manual policy only when the product must show competing edits to a person or
apply its own domain rule.

## Override only the policy you need

This Notebook asks for Manual handling when two devices change the same field.
The delete policy keeps its automatic default because it is not overridden.

@include sdks/js/guide/src/fundamentals/manual-conflicts.ts as src/notebook.ts

Manual does not turn every concurrent change into a conflict. Changes to
different top-level fields still combine automatically. `State.Conflicted`
appears only when the declared policy needs an application decision.

## Create a conflict in a test

`App.testWorld()` can model two installations of the same identity without
running two daemons. Named devices change their local copies independently;
`world.sync()` exchanges their changes deterministically.

@include sdks/js/guide/src/fundamentals/manual-conflict-usage.ts as src/resolve-conflict.ts

The normal Item state check narrows the result to a `ConflictItem`.
`alternatives` then contains immutable Present or Deleted possibilities. Each
alternative has `isPresent()` and `isDeleted()` helpers before application code
reads its value or chooses it.

## Choose or resolve

Use `conflict.choose(alternative)` when one exact alternative should win. Use
`conflict.resolve(value)` when the application creates a new combined
value. A custom value must pass the current Schema Class.

Both operations return a new immutable Item. Keep that returned Item just as
you would after an ordinary update. If another resolution wins first, the stale
Conflict Item throws `ConflictError`; read the Item again before deciding what
the interface should do.

## Manual deletion is separate

Set `conflicts: { delete: DeleteConflict.Manual }` only when a concurrent
deletion and update also needs a product decision. The alternatives can then
include a Deleted alternative. Choosing that alternative keeps the deletion;
choosing a Present alternative keeps its value.

An application may override update handling, delete handling, or both. Any
omitted policy retains the automatic default.

## Keep this out of the beginner path

Manual resolution adds user-interface states and domain decisions. Chirp does
not need it: its Resource definitions keep automatic handling and never expose
`State.Conflicted`, `alternatives`, `choose`, or `resolve` to the React code.

## Continue learning

- Use the normal lifecycle in [Change an Item](data-sdk-mutations.html).
- Return to [Data SDK fundamentals](data-sdk.html).
- Look up exact policy types in the [API reference](../sdk/reference.html#data.UpdateConflict).
