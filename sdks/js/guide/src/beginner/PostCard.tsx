import { useEffect, useState } from "react";

import type { TimelinePost } from "./timeline";
import type { ChirpProfile } from "./profiles";

type PostCardProps = {
  post: TimelinePost;
  profile?: ChirpProfile;
  ownPost: boolean;
  onDelete(post: TimelinePost): Promise<void>;
  onUpdate(post: TimelinePost, text: string): Promise<void>;
};

export function PostCard({
  post,
  profile,
  ownPost,
  onDelete,
  onUpdate,
}: PostCardProps) {
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(post.value.text);

  useEffect(() => {
    if (!editing) setText(post.value.text);
  }, [editing, post.value.text]);

  const save = async () => {
    const nextText = text.trim();
    if (!nextText) return;
    await onUpdate(post, nextText);
    setEditing(false);
  };

  return (
    <article className="post">
      <header className="post__meta">
        <div className="post__author">
          <strong>{profile?.nickname ?? post.ref.identity}</strong>
          {profile?.nickname && <span>{post.ref.identity}</span>}
        </div>
        <time dateTime={post.value.postedAt.toISOString()}>
          {post.value.postedAt.toLocaleString()}
        </time>
      </header>

      {editing ? (
        <form
          className="post__editor"
          onSubmit={(event) => {
            event.preventDefault();
            void save();
          }}
        >
          <textarea value={text} onChange={event => setText(event.target.value)} />
          <div className="post__actions">
            <button type="button" className="button button--quiet" onClick={() => setEditing(false)}>
              Cancel
            </button>
            <button type="submit" className="button">Save</button>
          </div>
        </form>
      ) : (
        <p className="post__text">{post.value.text}</p>
      )}

      {ownPost && !editing && (
        <footer className="post__actions">
          <button type="button" className="button button--quiet" onClick={() => setEditing(true)}>
            Edit
          </button>
          <button type="button" className="button button--danger" onClick={() => void onDelete(post)}>
            Delete
          </button>
        </footer>
      )}
    </article>
  );
}
