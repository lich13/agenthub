import type { HiddenScanRequest, HiddenThreadDeletePlan } from "../../types";

type Send = (request: HiddenScanRequest) => Promise<HiddenThreadDeletePlan>;

export function mergeHiddenScanPage(current: HiddenThreadDeletePlan | null, page: HiddenThreadDeletePlan): HiddenThreadDeletePlan {
  const scan = page.scan;
  if (!scan) return page;
  if (current?.scan && current.scan.id !== scan.id) throw new Error("扫描身份已变化，请重新扫描");
  const previous = current?.candidates ?? [];
  if (scan.cursor !== previous.length || scan.nextCursor !== scan.cursor + page.candidates.length
    || scan.nextCursor > scan.processed || scan.processed > scan.total
    || (scan.hasMore && page.candidates.length === 0)
    || (scan.phase === "complete" && scan.processed !== scan.total)) throw new Error("扫描游标未正确推进，请重新扫描");
  const candidates = previous.concat(page.candidates);
  return { ...page, candidates, hidden_ids: candidates.map(candidate => candidate.id) };
}

function pause(signal: AbortSignal): Promise<void> {
  return new Promise(resolve => {
    const finish = () => { clearTimeout(timer); signal.removeEventListener("abort", finish); resolve(); };
    const timer = setTimeout(finish, 250);
    signal.addEventListener("abort", finish, { once: true });
    if (signal.aborted) finish();
  });
}

export class HiddenScanRunner {
  private controller = new AbortController();
  private plan: HiddenThreadDeletePlan | null = null;
  private cancelRequest: Promise<void> | null = null;
  constructor(private send: Send, private update: (plan: HiddenThreadDeletePlan) => void) {}

  stop(): Promise<void> {
    this.controller.abort();
    const id = this.plan?.scan?.id;
    if (!id) return Promise.resolve();
    return this.cancelRequest ??= this.send({ mode: "cancel", scanId: id, cursor: this.plan!.candidates.length, limit: 100 })
      .then(() => undefined, () => undefined);
  }

  async run(): Promise<HiddenThreadDeletePlan> {
    let response = await this.send({ mode: "start", limit: 100 });
    while (true) {
      if (this.controller.signal.aborted) {
        // Retain only rows already shown. A late start still supplies the ID to cancel.
        this.plan ??= { ...response, candidates: [], hidden_ids: [] };
        await this.stop();
        if (this.plan.scan) this.plan = { ...this.plan, scan: { ...this.plan.scan, phase: "stopped" } };
        return this.plan;
      }
      this.plan = mergeHiddenScanPage(this.plan, response);
      this.update(this.plan);
      const scan = this.plan.scan;
      if (!scan) return this.plan;
      if (scan.phase === "failed") throw new Error(scan.error || "隐藏线程扫描失败");
      if ((scan.phase === "complete" && !scan.hasMore) || scan.phase === "stopped") return this.plan;
      if (!scan.hasMore) await pause(this.controller.signal);
      if (this.controller.signal.aborted) {
        await this.stop();
        return { ...this.plan, scan: { ...scan, phase: "stopped" } };
      }
      response = await this.send({ mode: "poll", scanId: scan.id, cursor: this.plan.candidates.length, limit: 100 });
    }
  }
}
