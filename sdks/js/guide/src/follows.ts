import { makeId } from "jolt-sdk";
import type { JoltIngressSdk } from "jolt-sdk";

export type FollowRequest = {
  kind: "chirp.follow-request";
  from: string;
  note?: string;
};

export function decodeFollowRequest(value: unknown): FollowRequest | null {
  if (typeof value !== "object" || value === null) return null;
  const v = value as Record<string, unknown>;
  return v.kind === "chirp.follow-request" && typeof v.from === "string"
    ? {
        kind: "chirp.follow-request",
        from: v.from,
        note: typeof v.note === "string" ? v.note : undefined,
      }
    : null;
}

export async function sendFollowRequest(
  jolt: JoltIngressSdk,
  me: string,
  them: string,
  note?: string
) {
  const request: FollowRequest = { kind: "chirp.follow-request", from: me, note };
  await jolt.sendObject(them, `/chirp/outbox/${makeId("follow")}`, request);
}

export type PendingFollow = { ingressId: string; request: FollowRequest };

export async function listFollowRequests(jolt: JoltIngressSdk): Promise<PendingFollow[]> {
  const pending: PendingFollow[] = [];
  for (const record of await jolt.listPendingIngress()) {
    const payload = await jolt.openIngress(record.ingress_id);
    const request = decodeFollowRequest(payload);
    if (!request) continue; // not a Chirp object; leave it pending for its app
    if (request.from !== record.sender_identity) {
      await jolt.rejectIngress(record.ingress_id); // claimed sender must match the envelope
      continue;
    }
    pending.push({ ingressId: record.ingress_id, request });
  }
  return pending;
}
