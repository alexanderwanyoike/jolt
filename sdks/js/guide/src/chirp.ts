import { makeId } from "jolt-sdk";
import type { JoltAppendSdk, JoltAvailabilitySdk, JoltSdk } from "jolt-sdk";

export type Chirp = {
  kind: "chirp";
  id: string;
  text: string;
  postedAt: string; // ISO 8601
};

export function decodeChirp(value: unknown): Chirp | null {
  if (typeof value !== "object" || value === null) return null;
  const v = value as Record<string, unknown>;
  return v.kind === "chirp" &&
    typeof v.id === "string" &&
    typeof v.text === "string" &&
    typeof v.postedAt === "string"
    ? { kind: "chirp", id: v.id, text: v.text, postedAt: v.postedAt }
    : null;
}

export async function postChirp(
  jolt: JoltAppendSdk,
  text: string,
  now: () => string = () => new Date().toISOString()
) {
  const id = makeId("chirp");
  const chirp: Chirp = { kind: "chirp", id, text, postedAt: now() };
  return jolt.publishAppend(`/chirp/posts/${id}`, chirp);
}

/** Publish, then explicitly request delegated availability from the home relay. */
export async function postAvailableChirp(
  jolt: JoltAppendSdk & JoltAvailabilitySdk,
  text: string,
  now: () => string = () => new Date().toISOString()
) {
  const published = await postChirp(jolt, text, now);
  await jolt.pinHomeRelay(published.contentId, published.path ?? undefined);
  return published;
}

export type TimelineEntry = { author: string; chirp: Chirp };

export async function loadTimeline(
  jolt: JoltSdk & JoltAppendSdk,
  identities: string[]
): Promise<TimelineEntry[]> {
  const entries: TimelineEntry[] = [];
  for (const identity of identities) {
    const records = await jolt.enumerate(identity, "/chirp/posts/");
    for (const record of records) {
      const read = await jolt.readContent(
        record.contentId,
        { identity, path: record.path },
        record.deviceSequence,
        decodeChirp
      );
      if (read) entries.push({ author: identity, chirp: read.value });
    }
  }
  return entries.sort((a, b) => b.chirp.postedAt.localeCompare(a.chirp.postedAt));
}

export type Follows = { kind: "chirp.follows"; identities: string[] };

export function decodeFollows(value: unknown): Follows | null {
  if (typeof value !== "object" || value === null) return null;
  const v = value as Record<string, unknown>;
  return v.kind === "chirp.follows" &&
    Array.isArray(v.identities) &&
    v.identities.every((entry) => typeof entry === "string")
    ? { kind: "chirp.follows", identities: v.identities as string[] }
    : null;
}

export async function loadFollows(jolt: JoltSdk, me: string): Promise<string[]> {
  const current = await jolt.read({ identity: me, path: "/chirp/follows" }, decodeFollows);
  return current?.value.identities ?? [];
}

export async function follow(jolt: JoltSdk, me: string, them: string) {
  const identities = new Set(await loadFollows(jolt, me));
  identities.add(them);
  await jolt.publishJson("/chirp/follows", {
    kind: "chirp.follows",
    identities: [...identities].sort(),
  });
}
