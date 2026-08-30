import {
  ChangeType,
  Subscription,
  type DataChangeStream,
  type DataSubscriptionChange,
  type DataSubscription,
} from "jolt-sdk/data";

import type { ChirpApplication, Post } from "./chirp";

type PostSubscription = DataSubscription<Post>;
export type TimelinePost = Awaited<ReturnType<PostSubscription["get"]>>[number];

export type TimelineSnapshot = {
  readonly posts: readonly TimelinePost[];
  readonly error: unknown;
};

type TimelineListener = (snapshot: TimelineSnapshot) => void;

type TimelineSource = {
  readonly subscription: PostSubscription;
  readonly stream: DataChangeStream<Post>;
  items: Map<string, TimelinePost>;
};

export function postKey(post: Pick<TimelinePost, "ref">): string {
  return `${post.ref.identity}${post.ref.path}`;
}

function itemsByRef(items: readonly TimelinePost[]): Map<string, TimelinePost> {
  return new Map(items.map(item => [postKey(item), item]));
}

export class Timeline {
  private readonly sources = new Map<string, TimelineSource>();
  private readonly listeners = new Set<TimelineListener>();
  private snapshot: TimelineSnapshot = Object.freeze({
    posts: Object.freeze([]),
    error: null,
  });
  private closed = false;

  private constructor(private readonly posts: ChirpApplication["posts"]) {}

  static async open(
    posts: ChirpApplication["posts"],
    identities: readonly string[],
  ): Promise<Timeline> {
    const timeline = new Timeline(posts);
    try {
      for (const identity of new Set(identities)) await timeline.add(identity);
      return timeline;
    } catch (error) {
      await timeline.close();
      throw error;
    }
  }

  getSnapshot = (): TimelineSnapshot => this.snapshot;

  subscribe(listener: TimelineListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    await Promise.all([...this.sources.values()].map(source => source.stream.cancel()));
    this.listeners.clear();
  }

  private async add(identity: string): Promise<void> {
    const subscription = await Subscription.create(this.posts.for(identity));
    const source: TimelineSource = {
      subscription,
      stream: subscription.changes(),
      items: new Map(),
    };
    this.sources.set(identity, source);

    // A Change Stream always begins with Jolt's retained verified Snapshot.
    // Await it before listening for deltas so an older parallel read can never
    // replace a newer post that has already arrived through the stream.
    const changes = source.stream[Symbol.asyncIterator]();
    const initial = await changes.next();
    if (initial.done || initial.value.type !== ChangeType.Snapshot) {
      throw new Error("Data Subscription did not begin with a Snapshot");
    }
    source.items = itemsByRef(initial.value.items);
    this.publish();
    void this.watch(source, changes).catch(error => this.fail(error));
  }

  private async refreshSource(source: TimelineSource): Promise<void> {
    source.items = itemsByRef(await source.subscription.get());
    this.publish();
  }

  private async watch(
    source: TimelineSource,
    changes: AsyncIterator<DataSubscriptionChange<Post>>,
  ): Promise<void> {
    while (true) {
      const event = await changes.next();
      if (event.done) return;
      const change = event.value;
      if (this.closed) return;

      switch (change.type) {
        case ChangeType.Snapshot:
          source.items = itemsByRef(change.items);
          this.publish();
          break;
        case ChangeType.Changed:
          for (const item of change.items) source.items.set(postKey(item), item);
          for (const ref of change.removed) {
            source.items.delete(postKey({ ref }));
          }
          this.publish();
          break;
        case ChangeType.ResyncRequired:
          await this.refreshSource(source);
          break;
        case ChangeType.State:
          break;
        case ChangeType.Cancelled:
        case ChangeType.Revoked:
          // A terminal stream can no longer keep this person's view current.
          source.items.clear();
          this.publish();
          return;
      }
    }
  }

  private publish(): void {
    const posts = [...this.sources.values()]
      .flatMap(source => [...source.items.values()])
      .sort((left, right) => right.value.postedAt.getTime() - left.value.postedAt.getTime());
    this.snapshot = Object.freeze({
      posts: Object.freeze(posts),
      error: this.snapshot.error,
    });
    for (const listener of this.listeners) listener(this.snapshot);
  }

  private fail(error: unknown): void {
    this.snapshot = Object.freeze({ ...this.snapshot, error });
    for (const listener of this.listeners) listener(this.snapshot);
  }
}
