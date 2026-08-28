import { State } from "jolt-sdk/data";

import { Chirp, type ChirpApplication } from "./chirp";

export async function exercisePostLifecycle(
  chirp: ChirpApplication,
  now: Date = new Date(),
) {
  const created = await chirp.posts.create({
    text: "Hello, Jolt!",
    postedAt: now,
  });

  const found = await chirp.posts.get(created.ref);
  if (found.state !== State.Present || !found.isPresent()) {
    throw new Error("The new post should be present");
  }

  const updated = await found.update({ text: "Hello, everyone!" });
  const deleted = await updated.delete();
  if (!deleted.isDeleted()) {
    throw new Error("The post should be deleted");
  }

  return deleted.restore({
    text: "Hello again!",
    postedAt: now,
  });
}

export async function exerciseConnectedPostLifecycle() {
  const chirp = await Chirp.connect();
  return exercisePostLifecycle(chirp);
}
