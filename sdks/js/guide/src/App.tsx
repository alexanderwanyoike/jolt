import { useCallback, useEffect, useState } from "react";

import { connect, jolt } from "./jolt";
import { follow, loadFollows, loadTimeline, postChirp } from "./chirp";
import type { TimelineEntry } from "./chirp";
import { listFollowRequests, sendFollowRequest } from "./follows";
import type { PendingFollow } from "./follows";
import "./App.css";

export default function App() {
  const [me, setMe] = useState<string | null>(null);
  const [timeline, setTimeline] = useState<TimelineEntry[]>([]);
  const [inbox, setInbox] = useState<PendingFollow[]>([]);
  const [draft, setDraft] = useState("");
  const [friend, setFriend] = useState("");
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async (identity: string) => {
    const follows = await loadFollows(jolt, identity);
    setTimeline(await loadTimeline(jolt, [identity, ...follows]));
    setInbox(await listFollowRequests(jolt));
  }, []);

  useEffect(() => {
    connect()
      .then(async (identity) => {
        setMe(identity);
        await refresh(identity);
      })
      .catch((cause) => setError(String(cause)));
  }, [refresh]);

  if (error) return <main className="chirp"><p className="error">{error}</p></main>;
  if (!me) return <main className="chirp"><p>Waiting for approval in Jolt Console…</p></main>;

  const run = (action: () => Promise<void>) =>
    action().then(() => refresh(me)).catch((cause) => setError(String(cause)));

  return (
    <main className="chirp">
      <header>
        <h1>Chirp</h1>
        <p className="identity">{me}</p>
      </header>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!draft.trim()) return;
          run(async () => {
            await postChirp(jolt, draft.trim());
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
