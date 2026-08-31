# Keep a remote Collection current

```meta
Guide: 06
Kicker: JOLT DATA SDK GUIDE
Level: Fundamentals
SDK: jolt-sdk/data
Description: Open a retained Data Subscription and receive local Change Stream events without application polling.
```

A Data Subscription keeps a verified local view of one identity's public
Collection. Application code can show retained data immediately, while Jolt
refreshes it within bounded network and storage limits.

## Start with a public Collection

Only a Resource using `Read.AnyIdentity` has `.for(identity)`, because only
that Resource can be read through another identity's public view.

@include sdks/js/guide/src/fundamentals/subscriptions.ts as src/feed.ts

## Create the Subscription

Pass the remote Collection view to `Subscription.create()`:

@include sdks/js/guide/src/fundamentals/subscription-usage.ts as src/open-author-posts.ts

`subscription.get()` returns the current retained verified Items. It does not
make the application enumerate the network. Calling `Subscription.create()`
again for the same authorized target safely reuses the daemon's bounded
subscription work.

`App.connect()` derives the required subscription access for Jolt Console
approval. If the node cannot admit more retained views, creation throws
`SubscriptionCapacityError`.

## Listen without polling

A Change Stream is an async sequence of changes to the local retained view. A
new stream starts with a full `ChangeType.Snapshot`, then reports changes and
freshness transitions.

@include sdks/js/guide/src/fundamentals/change-stream-usage.ts as src/watch-posts.ts

The example rereads the complete local view after `Changed` or
`ResyncRequired`. That is the simplest correct choice for one small
subscription. A larger feed can instead apply `change.items` and
`change.removed` to its own map, as Chirp does.

There is no timer in this code. Jolt wakes the local stream when verified data
or its freshness changes.

## Understand freshness

`subscription.state` and `State` on an Item answer different questions. Item
state says whether one logical record is Present, Deleted, Missing, or
Unavailable. `SubscriptionState` says how current the retained Collection view
is:

- `Loading`: the application has not received a verified view yet.
- `Updating`: a retained view is available while Jolt refreshes it.
- `Ready`: the latest refresh succeeded.
- `Stale`: retained data remains usable, but refresh failed; inspect `reason`.
- `Unavailable`: Jolt has no usable view at the moment.
- `Cancelled` and `Revoked`: the Subscription is terminal.

`lastVerifiedAt` records the last successful verification time when one is
available. A stale view is intentionally different from an empty Collection.

## Close the right thing

`stream.cancel()` stops one listener and leaves the retained Data Subscription
available. `subscription.remove()` removes the Subscription when the
application no longer wants Jolt to maintain that view.

Always close a Change Stream when its screen or controller stops. Treat
`ChangeType.Cancelled` and `ChangeType.Revoked` as terminal instead of starting
a hidden retry loop.

## Continue learning

- See this pattern inside the [beginner Chirp app](app-development.html).
- Return to [Data SDK fundamentals](data-sdk.html).
- Use the normal local lifecycle in [Change an Item](data-sdk-mutations.html).
