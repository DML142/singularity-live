import type { ReactNode } from "react";

interface WorkspacePanelProps {
  readonly title: string;
  readonly detail?: string;
  readonly children: ReactNode;
  readonly className?: string;
}

export function WorkspacePanel({
  title,
  detail,
  children,
  className = "",
}: WorkspacePanelProps) {
  return (
    <section className={`workspace-panel ${className}`.trim()}>
      <header className="panel-header">
        <h2>{title}</h2>
        {detail ? <span>{detail}</span> : null}
      </header>
      {children}
    </section>
  );
}
