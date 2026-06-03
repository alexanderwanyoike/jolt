import { Placeholder, SectionPanel } from "../components/primitives";

export function SettingsPage() {
  return (
    <SectionPanel eyebrow="Settings" summary="future daemon configuration" hero>
      <Placeholder>
        Settings are intentionally read-only in this shell. Identity import, relay configuration,
        and daemon lifecycle controls should be designed deliberately.
      </Placeholder>
    </SectionPanel>
  );
}
