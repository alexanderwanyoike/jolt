import { Subscription } from "jolt-sdk/data";
import { Feed } from "./subscriptions";

type FeedApplication = ReturnType<typeof Feed.test>;

export async function openAuthorPosts(
  feed: FeedApplication,
  identity: string,
) {
  const subscription = await Subscription.create(feed.posts.for(identity));
  const posts = await subscription.get();

  return { subscription, posts };
}
