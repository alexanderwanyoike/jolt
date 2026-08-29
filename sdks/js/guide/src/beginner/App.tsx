import { useEffect, useMemo, useState } from "react";

import {
  Chirp,
  type ChirpApplication,
  type DeletedChirpPost,
  type Post,
} from "./chirp";
import { follow, getFollowing, type FollowingItem } from "./following";
import { PostCard } from "./PostCard";
import type { TimelinePost } from "./timeline";
import { useTimeline } from "./use-timeline";
import "./App.css";

type DeletedPost = {
  item: DeletedChirpPost;
  value: Post;
};

export default function App() {
  const [chirp, setChirp] = useState<ChirpApplication | null>(null);
  const [following, setFollowing] = useState<FollowingItem | null>(null);
  const [draft, setDraft] = useState("");
  const [friend, setFriend] = useState("");
  const [deleted, setDeleted] = useState<DeletedPost | null>(null);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    let cancelled = false;
    void Chirp.connect()
      .then(async (connected) => ({ connected, following: await getFollowing(connected) }))
      .then((connection) => {
        if (cancelled) return;
        setChirp(connection.connected);
        setFollowing(connection.following);
      })
      .catch(error => setError(error));
    return () => { cancelled = true; };
  }, []);

  const identities = useMemo(
    () => chirp === null
      ? []
      : [chirp.identity, ...(following?.value.identities ?? [])],
    [chirp, following],
  );
  const timeline = useTimeline(chirp, identities);

  const run = async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (actionError) {
      setError(actionError);
    }
  };

  const createPost = async () => {
    if (chirp === null || !draft.trim()) return;
    await chirp.posts.create({ text: draft.trim(), postedAt: new Date() });
    setDraft("");
  };

  const addFriend = async () => {
    if (chirp === null || !friend.trim()) return;
    setFollowing(await follow(chirp, friend.trim()));
    setFriend("");
  };

  const updatePost = async (post: TimelinePost, text: string) => {
    if (chirp === null) return;
    const current = await chirp.posts.get(post.ref);
    if (current.isPresent()) await current.update({ text });
  };

  const deletePost = async (post: TimelinePost) => {
    if (chirp === null) return;
    const current = await chirp.posts.get(post.ref);
    if (!current.isPresent()) return;
    setDeleted({
      item: await current.delete(),
      value: {
        text: current.value.text,
        postedAt: current.value.postedAt,
      },
    });
  };

  const restorePost = async () => {
    if (deleted === null) return;
    await deleted.item.restore(deleted.value);
    setDeleted(null);
  };

  if (error !== null || timeline.error !== null) {
    return <main className="chirp-shell"><p className="notice notice--error">{String(error ?? timeline.error)}</p></main>;
  }

  if (chirp === null || following === null) {
    return (
      <main className="chirp-shell chirp-shell--centered">
        <p className="eyebrow">Chirp is meeting Jolt</p>
        <h1>Approve Chirp in Jolt Console</h1>
      </main>
    );
  }

  return (
    <main className="chirp-shell">
      <header className="masthead">
        <div>
          <p className="eyebrow">A small social app on Jolt</p>
          <h1>Chirp<span>.</span></h1>
        </div>
        <p className="identity">{chirp.identity}</p>
      </header>

      <section className="workspace">
        <div className="compose-column">
          <form
            className="composer paper"
            onSubmit={(event) => {
              event.preventDefault();
              void run(createPost);
            }}
          >
            <label htmlFor="chirp-text">What do you want to say?</label>
            <textarea
              id="chirp-text"
              value={draft}
              maxLength={280}
              placeholder="A thought worth sharing…"
              onChange={event => setDraft(event.target.value)}
            />
            <div className="composer__footer">
              <span>{draft.length}/280</span>
              <button className="button" type="submit">Publish chirp</button>
            </div>
          </form>

          <form
            className="follow-form paper"
            onSubmit={(event) => {
              event.preventDefault();
              void run(addFriend);
            }}
          >
            <label htmlFor="friend">Follow a Jolt identity</label>
            <div>
              <input
                id="friend"
                value={friend}
                placeholder="alice.jolt"
                onChange={event => setFriend(event.target.value)}
              />
              <button className="button button--ink" type="submit">Follow</button>
            </div>
          </form>
        </div>

        <section className="timeline" aria-labelledby="timeline-heading">
          <header className="timeline__heading">
            <div>
              <p className="eyebrow">Your network</p>
              <h2 id="timeline-heading">Latest chirps</h2>
            </div>
            <span>{timeline.posts.length} posts</span>
          </header>

          {timeline.posts.length === 0 ? (
            <div className="empty paper">
              <p>It is quiet here.</p>
              <span>Publish something or follow a friend.</span>
            </div>
          ) : timeline.posts.map(post => (
            <PostCard
              key={`${post.ref.identity}${post.ref.path}`}
              post={post}
              ownPost={post.ref.identity === chirp.identity}
              onUpdate={(post, text) => run(() => updatePost(post, text))}
              onDelete={post => run(() => deletePost(post))}
            />
          ))}
        </section>
      </section>

      {deleted !== null && (
        <aside className="undo" role="status">
          <span>Chirp deleted.</span>
          <button type="button" onClick={() => void run(restorePost)}>Undo</button>
        </aside>
      )}
    </main>
  );
}
