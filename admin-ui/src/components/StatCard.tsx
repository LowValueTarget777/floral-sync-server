type StatCardProps = {
  label: string;
  value: string | number;
  detail: string;
};

export function StatCard({ label, value, detail }: StatCardProps) {
  return (
    <article className="stat-card">
      <p className="section-label">{label}</p>
      <strong>{value}</strong>
      <p className="muted">{detail}</p>
    </article>
  );
}
