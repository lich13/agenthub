import type { ReactNode } from "react";

export function Panel({
  title,
  icon,
  action,
  children,
  className = ""
}: {
  title: string;
  icon: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`panel ${className}`}>
      <header>
        {icon}
        <strong>{title}</strong>
        {action}
      </header>
      {children}
    </section>
  );
}

export function Metric({
  label,
  value,
  tone,
  wide = false
}: {
  label: string;
  value: string;
  tone?: "success" | "warning" | "danger" | "info" | "neutral";
  wide?: boolean;
}) {
  return (
    <div className={wide ? "metric metric-wide" : "metric"}>
      <span>{label}</span>
      <strong className={tone ? `tone-${tone}` : ""}>{value}</strong>
    </div>
  );
}
