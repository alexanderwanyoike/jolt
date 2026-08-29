import { useEffect, useState } from "react";

import type { ChirpApplication } from "./chirp";
import { Timeline, type TimelineSnapshot } from "./timeline";

const emptyTimeline: TimelineSnapshot = Object.freeze({
  posts: Object.freeze([]),
  error: null,
});

export function useTimeline(
  chirp: ChirpApplication | null,
  identities: readonly string[],
): TimelineSnapshot {
  const [snapshot, setSnapshot] = useState(emptyTimeline);

  useEffect(() => {
    let timeline: Timeline | undefined;
    let unsubscribe: (() => void) | undefined;
    let cancelled = false;

    if (chirp === null) {
      setSnapshot(emptyTimeline);
      return;
    }

    void Timeline.open(chirp.posts, identities)
      .then((opened) => {
        if (cancelled) return opened.close();
        timeline = opened;
        setSnapshot(opened.getSnapshot());
        unsubscribe = opened.subscribe(setSnapshot);
      })
      .catch((error) => {
        if (!cancelled) setSnapshot({ posts: [], error });
      });

    return () => {
      cancelled = true;
      unsubscribe?.();
      void timeline?.close();
    };
  }, [chirp, identities]);

  return snapshot;
}
