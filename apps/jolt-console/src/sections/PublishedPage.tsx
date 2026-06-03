import { Placeholder, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { formatBytes, shortId } from "../utils/format";

export function PublishedPage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const count = snapshot.published.length;

  return (
    <SectionPanel eyebrow="Published" summary={`${count} ${count === 1 ? "item" : "items"}`} hero>
      {!snapshot.published.length ? (
        <Placeholder>No published content yet.</Placeholder>
      ) : (
        <div className="list-panel">
          {snapshot.published.slice(0, 8).map((item) => (
            <article className="content-row" key={item.content_id}>
              <div>
                <strong className="mono">{item.path ?? "unaddressed"}</strong>
                <span>
                  {formatBytes(item.size)} - {item.pin_state ?? "local_only"}
                </span>
              </div>
              <code>{shortId(item.content_id)}</code>
            </article>
          ))}
        </div>
      )}
    </SectionPanel>
  );
}
