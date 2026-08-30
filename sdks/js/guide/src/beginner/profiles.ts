import type { ChirpApplication } from "./chirp";

export type ChirpProfile = {
  readonly identity: string;
  readonly nickname?: string;
};

export async function saveNickname(
  chirp: ChirpApplication,
  nickname: string,
) {
  const trimmed = nickname.trim();
  if (!trimmed) throw new Error("Enter a nickname");

  const profile = await chirp.profile.getOrCreate({ nickname: trimmed });
  return profile.value.nickname === trimmed
    ? profile
    : profile.update({ nickname: trimmed });
}

export async function getProfiles(
  chirp: ChirpApplication,
  identities: readonly string[],
): Promise<ReadonlyMap<string, ChirpProfile>> {
  const profiles = await Promise.all(
    [...new Set(identities)].map(async (identity) => {
      const item = identity === chirp.identity
        ? await chirp.profile.get()
        : await chirp.profile.for(identity).get();
      const profile: ChirpProfile = item.isPresent()
        ? { identity, nickname: item.value.nickname }
        : { identity };
      return [identity, profile] as const;
    }),
  );
  return new Map(profiles);
}
