import type { ChirpApplication } from "./chirp";

export type FollowingItem = Awaited<
  ReturnType<ChirpApplication["following"]["getOrCreate"]>
>;

export async function getFollowing(
  chirp: ChirpApplication,
): Promise<FollowingItem> {
  return chirp.following.getOrCreate({ identities: [] });
}

export async function follow(
  chirp: ChirpApplication,
  identity: string,
): Promise<FollowingItem> {
  const following = await getFollowing(chirp);
  if (following.value.identities.includes(identity)) return following;

  return following.update({
    identities: [...following.value.identities, identity],
  });
}
