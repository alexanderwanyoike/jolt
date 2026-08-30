import { beforeEach, describe, expect, it, vi } from "vitest";

const host = vi.hoisted(() => ({
  connectDataApp: vi.fn(),
}));

vi.mock("../src/data-host.js", async importOriginal => ({
  ...await importOriginal<typeof import("../src/data-host.js")>(),
  connectDataApp: host.connectDataApp,
}));

import {
  App,
  Collection,
  Field,
  Read,
  Schema,
  type DataSdkClient,
} from "../src/data.js";

function deferred<T>(): {
  readonly promise: Promise<T>;
  readonly resolve: (value: T) => void;
  readonly reject: (reason: unknown) => void;
} {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

function createChirp() {
  @Schema({ version: 1 })
  class Post {
    @Field.string()
    text!: string;
  }

  return App.create({
    id: "chirp.example",
    name: "Chirp",
    namespace: "chirp",
    data: {
      posts: Collection.create(Post, {
        access: { read: Read.AnyIdentity, create: true },
      }),
    },
  });
}

const connection = {
  identity: "alice.jolt",
  client: {} as DataSdkClient,
};

describe("App connection bootstrap", () => {
  beforeEach(() => {
    host.connectDataApp.mockReset();
  });

  it("shares one approval flow between simultaneous App.connect calls", async () => {
    const pending = deferred<typeof connection>();
    host.connectDataApp.mockReturnValue(pending.promise);
    const Chirp = createChirp();

    const first = Chirp.connect();
    const second = Chirp.connect();

    expect(host.connectDataApp).toHaveBeenCalledOnce();
    pending.resolve(connection);
    await expect(Promise.all([first, second])).resolves.toMatchObject([
      { identity: "alice.jolt" },
      { identity: "alice.jolt" },
    ]);
  });

  it("allows a fresh connection attempt after shared approval fails", async () => {
    const pending = deferred<typeof connection>();
    const rejection = new Error("approval expired");
    host.connectDataApp
      .mockReturnValueOnce(pending.promise)
      .mockResolvedValueOnce(connection);
    const Chirp = createChirp();

    const first = Chirp.connect();
    const second = Chirp.connect();
    pending.reject(rejection);

    await expect(first).rejects.toBe(rejection);
    await expect(second).rejects.toBe(rejection);
    await expect(Chirp.connect()).resolves.toMatchObject({ identity: "alice.jolt" });
    expect(host.connectDataApp).toHaveBeenCalledTimes(2);
  });
});
