import {
  ChangeType,
  type DataSubscription,
  type PresentItem,
  type SubscriptionStateValue,
} from "jolt-sdk/data";
import type { Post } from "./subscriptions";

type PostItem = PresentItem<Post>;
type ViewListener = (posts: readonly PostItem[]) => void;
type StateListener = (state: SubscriptionStateValue) => void;

export function watchPosts(
  subscription: DataSubscription<Post>,
  showPosts: ViewListener,
  showState: StateListener,
) {
  const stream = subscription.changes();
  const done = (async () => {
    try {
      for await (const change of stream) {
        switch (change.type) {
          case ChangeType.Snapshot:
            showPosts(change.items);
            showState(change.state);
            break;
          case ChangeType.Changed:
          case ChangeType.ResyncRequired:
            showPosts(await subscription.get());
            break;
          case ChangeType.State:
            showState(change.state);
            break;
          case ChangeType.Cancelled:
          case ChangeType.Revoked:
            return;
        }
      }
    } finally {
      await stream.cancel();
    }
  })();

  return Object.freeze({
    done,
    async close() {
      await stream.cancel();
      await done;
    },
  });
}
