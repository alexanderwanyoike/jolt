import { useEffect, useMemo, useState } from "react";
import { AppIncompatibleError } from "jolt-sdk/data";

import {
  Chirp,
  type ChirpApplication,
  type ChirpPost,
  type DeletedChirpPost,
} from "./chirp";
import { follow, getFollowing, type FollowingItem } from "./following";
import { PostCard } from "./PostCard";
import {
  getProfiles,
  saveNickname,
  type ChirpProfile,
} from "./profiles";
import { postKey, type TimelinePost } from "./timeline";
import { useTimeline } from "./use-timeline";
import "./App.css";

type DeletedPost = {
  deleted: DeletedChirpPost;
  previous: ChirpPost;
};

type StartupFailure = {
  title: string;
  message: string;
};

export function describeStartupFailure(error: unknown): StartupFailure {
  if (error instanceof AppIncompatibleError) {
    return {
      title: "Chirp needs a newer Jolt",
      message:
        "Update Jolt Console, then choose Check again. Chirp stopped before requesting access or changing data.",
    };
  }
  return {
    title: "Chirp could not start",
    message: error instanceof Error ? error.message : "Please try again.",
  };
}

export default function App() {
  const [chirp, setChirp] = useState<ChirpApplication | null>(null);
  const [following, setFollowing] = useState<FollowingItem | null>(null);
  const [profiles, setProfiles] = useState<ReadonlyMap<string, ChirpProfile>>(
    () => new Map(),
  );
  const [nickname, setNickname] = useState("");
  const [draft, setDraft] = useState("");
  const [friend, setFriend] = useState("");
  const [deleted, setDeleted] = useState<DeletedPost | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [connectionAttempt, setConnectionAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    void Chirp.connect()
      .then(async (connected) => ({ connected, following: await getFollowing(connected) }))
      .then((connection) => {
        if (cancelled) return;
        setChirp(connection.connected);
        setFollowing(connection.following);
      })
      .catch(error => {
        if (!cancelled) setError(error);
      });
    return () => { cancelled = true; };
  }, [connectionAttempt]);

  const identities = useMemo(
    () => chirp === null
      ? []
      : [chirp.identity, ...(following?.value.identities ?? [])],
    [chirp, following],
  );
  const timeline = useTimeline(chirp, identities);

  useEffect(() => {
    if (chirp === null) return;
    let cancelled = false;
    void getProfiles(chirp, identities)
      .then((loaded) => {
        if (cancelled) return;
        setProfiles(loaded);
        setNickname(current => current || (loaded.get(chirp.identity)?.nickname ?? ""));
      })
      .catch(profileError => {
        if (!cancelled) setError(profileError);
      });
    return () => { cancelled = true; };
  }, [chirp, identities, timeline.posts]);

  const run = async (action: () => Promise<void>) => {
    setError(null);
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

  const updateNickname = async () => {
    if (chirp === null) return;
    const saved = await saveNickname(chirp, nickname);
    setProfiles(current => new Map(current).set(chirp.identity, {
      identity: chirp.identity,
      nickname: saved.value.nickname,
    }));
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
    setDeleted({ deleted: await current.delete(), previous: current });
  };

  const restorePost = async () => {
    if (deleted === null) return;
    await deleted.deleted.restore(deleted.previous.value);
    setDeleted(null);
  };

  if (chirp === null && error !== null) {
    const failure = describeStartupFailure(error);
    return (
      <main className="chirp-shell chirp-shell--centered">
        <p className="eyebrow">Chirp could not meet Jolt</p>
        <h1>{failure.title}</h1>
        <p>{failure.message}</p>
        <button
          className="button"
          type="button"
          onClick={() => setConnectionAttempt(attempt => attempt + 1)}
        >
          Check again
        </button>
      </main>
    );
  }

  if (timeline.error !== null) {
    return <main className="chirp-shell"><p className="notice notice--error">{String(timeline.error)}</p></main>;
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

      {error !== null && (
        <aside className="notice notice--error" role="alert">
          <span>{String(error)}</span>
          <button type="button" onClick={() => setError(null)}>Dismiss</button>
        </aside>
      )}

      <section className="workspace">
        <div className="compose-column">
          <form
            className="profile-form paper"
            onSubmit={(event) => {
              event.preventDefault();
              void run(updateNickname);
            }}
          >
            <label htmlFor="nickname">Your nickname</label>
            <div>
              <input
                id="nickname"
                value={nickname}
                maxLength={40}
                placeholder="Alice"
                onChange={event => setNickname(event.target.value)}
              />
              <button className="button" type="submit">Save</button>
            </div>
          </form>

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

          <section className="following-list paper" aria-labelledby="following-heading">
            <header>
              <h2 id="following-heading">Following</h2>
              <span>{following.value.identities.length}</span>
            </header>
            {following.value.identities.length === 0 ? (
              <p>People you follow will appear here.</p>
            ) : (
              <ul>
                {following.value.identities.map((identity) => {
                  const profile = profiles.get(identity);
                  return (
                    <li key={identity}>
                      <strong>{profile?.nickname ?? identity}</strong>
                      {profile?.nickname && <span>{identity}</span>}
                    </li>
                  );
                })}
              </ul>
            )}
          </section>
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
              key={postKey(post)}
              post={post}
              profile={profiles.get(post.ref.identity)}
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
