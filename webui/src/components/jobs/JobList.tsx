import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getJob } from "../../lib/api/jobs";
import { machineScope } from "../../lib/query/connection";
import { ChevronRight } from "lucide-react";
import {
  jobFailureAnalysisView,
  jobOutputView,
  type RuntimeCapabilityInput
} from "../../lib/domain/runtimeViewModel";
import type { JobRecord } from "../../types";

export function JobList({ jobs, capabilities }: { jobs: JobRecord[]; capabilities: RuntimeCapabilityInput }) {
  return (
    <div className="job-list">
      {jobs.map(job => <JobItem key={job.id} job={job} capabilities={capabilities} />)}
      {jobs.length === 0 && <div className="muted-row">暂无后台 job</div>}
    </div>
  );
}

function StatusDot({ status }: { status: string }) {
  const label = ({ succeeded: "成功", failed: "失败", running: "进行中", queued: "排队中", cancelled: "已取消" } as Record<string, string>)[status] ?? status;
  return <><span className={`status-dot ${status}`} aria-hidden="true" /><small>{label}</small></>;
}

function JobItem({job: summary, capabilities}: {job: JobRecord; capabilities: RuntimeCapabilityInput}) {
  const [open, setOpen] = useState(false);
  const detail = useQuery({queryKey: ["job-output", machineScope(), summary.id], queryFn: () => getJob(summary.id), enabled: open,
    staleTime: 0,
    refetchInterval: query => open && (summary.status === "running" || query.state.data?.status === "running") ? 2000 : false});
  const job = detail.data ?? summary;
  const analysis = job.failure_analysis ? jobFailureAnalysisView(job.failure_analysis, capabilities) : null;
  return <details className="job-item" open={open} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary><span>{summary.title}</span><StatusDot status={summary.status}/><ChevronRight size={16}/></summary>
    {open && <><div className="job-meta"><span>{job.kind}</span>{job.thread_id && <span>{job.thread_id}</span>}{job.turn_id && <span>{job.turn_id}</span>}</div>
    {analysis && <div className="job-analysis"><strong>{analysis.label}</strong><p>{analysis.explanation}</p><ul>{analysis.suggestions.map(value => <li key={value}>{value}</li>)}</ul></div>}
    {detail.error ? <div role="alert">{detail.error.message}</div> : <pre>{detail.isLoading ? "正在读取日志…" : jobOutputView(job.output || job.error || "no output", capabilities)}</pre>}</>}
  </details>;
}
