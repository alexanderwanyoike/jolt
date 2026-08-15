import { useCallback, useEffect, useState } from "react";

import { connect, jolt } from "./jolt";
import type { ChirpConnectResult } from "./jolt";
import {
  follow,
  loadFollows,
  loadTimeline,
  postAvailableChirp,
  postChirp,
} from "./chirp";
import type { TimelineEntry } from "./chirp";
import { listFollowRequests, sendFollowRequest } from "./follows";
import type { PendingFollow } from "./follows";
import "./App.css";

export default function App() {
  const [connection, setConnection] = useState<ChirpConnectResult | null>(null);
  const [timeline, setTimeline] = useState<TimelineEntry[]>([]);
  const [inbox, setInbox] = useState<PendingFollow[]>([]);
  const [draft, setDraft] = useState("");
  const [keepAvailable, setKeepAvailable] = useState(false);
  const [friend, setFriend] = useState("");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (identity: string) => {
    const follows = await loadFollows(jolt, identity);
    setTimeline(await loadTimeline(jolt, [identity, ...follows]));
    setInbox(await listFollowRequests(jolt));
  }, []);

  const checkConnection = useCallback(async () => {
    setConnection(null);
    setError(null);
    try {
      const result = await connect();
      setConnection(result);
      if (result.status === "ready") await refresh(result.identity);
    } catch (cause) {
      setError(String(cause));
    }
  }, [refresh]);

  useEffect(() => {
    void checkConnection();
  }, [checkConnection]);

  if (error) {
    return (
      <main className="chirp">
        <p className="error">{error}</p>
        <button type="button" onClick={() => void checkConnection()}>Check again</button>
      </main>
    );
  }
  if (!connection) {
    return <main className="chirp"><p>Checking Jolt and waiting for approval…</p></main>;
  }
  if (connection.status === "unavailable") {
    return (
      <main className="chirp">
        <h1>Jolt is unavailable</h1>
        <p>{connection.message}</p>
        <button type="button" onClick={() => void checkConnection()}>Check again</button>
      </main>
    );
  }
  if (connection.status === "incompatible") {
    const appApiMissing =
      connection.availableAppApi === null ||
      connection.availableAppApi < connection.requiredAppApi;
    return (
      <main className="chirp">
        <h1>Upgrade Jolt to run this Chirp release</h1>
        {appApiMissing ? (
          <p>
            Chirp needs App API {connection.requiredAppApi}; this Jolt daemon provides {" "}
            {connection.availableAppApi ?? "no compatible App API"}.
          </p>
        ) : (
          <p>The App API level is compatible, but Jolt is missing required behavior:</p>
        )}
        {connection.missingRequiredFeatures.length > 0 && (
          <ul>
            {connection.missingRequiredFeatures.map((feature) => (
              <li key={feature.feature}>
                {feature.feature} level {feature.requiredLevel} required; {" "}
                {feature.availableLevel === null
                  ? "not advertised"
                  : `level ${feature.availableLevel} available`}
              </li>
            ))}
          </ul>
        )}
        <button type="button" onClick={() => void checkConnection()}>Check again</button>
      </main>
    );
  }

  const me = connection.identity;
  const canKeepAvailable =
    connection.homeRelayAvailability === "available" && connection.homeRelayAuthorized;

  const run = (action: () => Promise<void>) =>
    action().then(() => refresh(me)).catch((cause) => setError(String(cause)));

  return (
    <main className="chirp">
      <header>
        <h1>Chirp</h1>
        <p className="identity">{me}</p>
      </header>

      <form
        className="composer"
        onSubmit={(event) => {
          event.preventDefault();
          if (!draft.trim()) return;
          run(async () => {
            if (keepAvailable && canKeepAvailable) {
              await postAvailableChirp(jolt, draft.trim());
            } else {
              await postChirp(jolt, draft.trim());
            }
            setDraft("");
          });
        }}
      >
        <textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="What's happening on the network?"
          maxLength={280}
        />
        {canKeepAvailable && (
          <label className="availability">
            <input
              type="checkbox"
              checked={keepAvailable}
              onChange={(event) => setKeepAvailable(event.target.checked)}
            />
            Keep this chirp available through my home relay
          </label>
        )}
        <button type="submit">Chirp</button>
      </form>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!friend.trim()) return;
          run(async () => {
            const them = friend.trim();
            await follow(jolt, me, them); // their posts are public; just subscribe
            await sendFollowRequest(jolt, me, them, "chirp?"); // and say hello
            setFriend("");
          });
        }}
      >
        <input
          value={friend}
          onChange={(event) => setFriend(event.target.value)}
          placeholder="somebody.jolt"
        />
        <button type="submit">Follow</button>
      </form>

      {inbox.length > 0 && (
        <section>
          <h2>Follow requests</h2>
          {inbox.map((pending) => (
            <article key={pending.ingressId} className="request">
              <span>
                <strong>{pending.request.from}</strong> {pending.request.note ?? ""}
              </span>
              <button onClick={() => run(async () => {
                await jolt.acceptIngress(pending.ingressId);
                await follow(jolt, me, pending.request.from); // follow back
              })}>
                Accept
              </button>
              <button onClick={() => run(() => jolt.rejectIngress(pending.ingressId))}>
                Ignore
              </button>
            </article>
          ))}
        </section>
      )}

      <section>
        <h2>Timeline</h2>
        {timeline.length === 0 && <p>No chirps yet. Write the first one.</p>}
        {timeline.map((entry) => (
          <article key={entry.chirp.id}>
            <header>
              <strong>{entry.author}</strong>
              <time dateTime={entry.chirp.postedAt}>
                {new Date(entry.chirp.postedAt).toLocaleString()}
              </time>
            </header>
            <p>{entry.chirp.text}</p>
          </article>
        ))}
      </section>
    </main>
  );
}
