import { Placeholder, SectionPanel } from "../components/primitives";

export function AppsPage() {
  return (
    <SectionPanel eyebrow="Apps" summary="permission approvals land next" hero>
      <Placeholder>
        App session approval will use <span className="mono">/admin/v1/app-requests</span> and{" "}
        <span className="mono">/admin/v1/app-sessions</span>. This shell reserves the trust surface
        for approving, rejecting, and revoking external app access.
      </Placeholder>
    </SectionPanel>
  );
}
