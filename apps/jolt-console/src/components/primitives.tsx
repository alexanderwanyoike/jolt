type SectionPanelProps = {
  children: React.ReactNode;
  eyebrow: string;
  summary: string;
  hero?: boolean;
};

type MetricCardProps = {
  label: string;
  value: string;
};

type DetailRowProps = {
  label: string;
  value: string;
};

export function SectionPanel({ children, eyebrow, summary, hero = false }: SectionPanelProps) {
  return (
    <section className={`section-panel ${hero ? "hero-panel" : ""}`}>
      <div className="section-title">
        <span>{eyebrow}</span>
        <strong>{summary}</strong>
      </div>
      {children}
    </section>
  );
}

export function MetricGrid({ children }: { children: React.ReactNode }) {
  return <div className="metric-grid">{children}</div>;
}

export function DetailGrid({ children }: { children: React.ReactNode }) {
  return <div className="detail-grid">{children}</div>;
}

export function MetricCard({ label, value }: MetricCardProps) {
  return (
    <div className="metric-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

export function DetailRow({ label, value }: DetailRowProps) {
  return (
    <div className="detail-row">
      <span>{label}</span>
      <strong className="mono">{value}</strong>
    </div>
  );
}

export function Placeholder({ children }: { children: React.ReactNode }) {
  return <div className="placeholder">{children}</div>;
}
