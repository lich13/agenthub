import { describe, expect, test, vi } from "vitest";
import type {
  HiddenScanProgress,
  HiddenScanRequest,
  HiddenThreadCandidate,
  HiddenThreadDeletePlan
} from "../../types";
import { HiddenScanRunner, mergeHiddenScanPage } from "./hiddenScan";

function candidate(id: string): HiddenThreadCandidate {
  return { id, title: id, fingerprint: "fixture-" + id, allowed: true, reason: null };
}

function progress(overrides: Partial<HiddenScanProgress> = {}): HiddenScanProgress {
  return {
    id: "scan-fixture",
    phase: "scanning",
    processed: 0,
    total: 2,
    cursor: 0,
    nextCursor: 0,
    hasMore: true,
    error: null,
    ...overrides
  };
}

function scanPlan(candidates: HiddenThreadCandidate[], scan: HiddenScanProgress): HiddenThreadDeletePlan {
  return {
    total_threads: scan.total,
    visible_threads: 0,
    hidden_threads: scan.total,
    archived_threads: 0,
    session_index_lines: scan.total,
    rollout_files: scan.total,
    hidden_ids: candidates.map(item => item.id),
    candidates,
    hidden_source_counts: { fixture: candidates.length },
    integrity: "ok",
    scan
  };
}

function cancelResponse(): HiddenThreadDeletePlan {
  const candidates = [candidate("discarded-thread")];
  return {
    total_threads: 1,
    visible_threads: 0,
    hidden_threads: 1,
    archived_threads: 0,
    session_index_lines: 1,
    rollout_files: 1,
    hidden_ids: candidates.map(item => item.id),
    candidates,
    hidden_source_counts: { fixture: 1 },
    integrity: "ok"
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

describe("hidden thread scan paging", () => {
  test("missing progress fails without silently accepting a synchronous preview", async () => {
    const send = vi.fn(async () => cancelResponse());
    const update = vi.fn();
    await expect(new HiddenScanRunner(send, update).run()).rejects.toThrow("扫描响应缺少进度");
    expect(update).not.toHaveBeenCalled();
  });

  test("accumulates pages and polls from the next candidate cursor", async () => {
    const first = scanPlan([candidate("thread-a")], progress({ processed: 1, nextCursor: 1 }));
    const second = scanPlan(
      [candidate("thread-b")],
      progress({ phase: "complete", processed: 2, cursor: 1, nextCursor: 2, hasMore: false })
    );
    const send = vi.fn(async (request: HiddenScanRequest): Promise<HiddenThreadDeletePlan> => {
      if (request?.mode === "start") return first;
      if (request?.mode === "poll") return second;
      throw new Error("Unexpected scan request");
    });
    const updates: HiddenThreadDeletePlan[] = [];

    const result = await new HiddenScanRunner(send, plan => updates.push(plan)).run();

    expect(send).toHaveBeenNthCalledWith(1, { mode: "start", limit: 100 });
    expect(send).toHaveBeenNthCalledWith(2, { mode: "poll", scanId: "scan-fixture", cursor: 1, limit: 100 });
    expect(result.candidates.map(item => item.id)).toEqual(["thread-a", "thread-b"]);
    expect(result.hidden_ids).toEqual(["thread-a", "thread-b"]);
    expect(result.scan?.phase).toBe("complete");
    expect(updates.map(plan => plan.candidates.map(item => item.id))).toEqual([
      ["thread-a"],
      ["thread-a", "thread-b"]
    ]);
  });

  test("rejects a page whose scan identity or cursor does not match", () => {
    const current = scanPlan([candidate("thread-a")], progress({ processed: 1, nextCursor: 1 }));
    const wrongIdentity = scanPlan(
      [candidate("thread-b")],
      progress({ id: "other-scan", phase: "complete", processed: 2, cursor: 1, nextCursor: 2, hasMore: false })
    );
    const wrongCursor = scanPlan(
      [candidate("thread-b")],
      progress({ phase: "complete", processed: 2, cursor: 0, nextCursor: 1, hasMore: false })
    );

    expect(() => mergeHiddenScanPage(current, wrongIdentity)).toThrow("扫描身份已变化");
    expect(() => mergeHiddenScanPage(current, wrongCursor)).toThrow("扫描游标未正确推进");
  });

  test("rejects empty pages that claim more work and incomplete completed scans", () => {
    const current = scanPlan(
      [candidate("thread-a")],
      progress({ processed: 1, total: 3, nextCursor: 1 })
    );
    const emptyPageWithMore = scanPlan(
      [],
      progress({ processed: 1, total: 3, cursor: 1, nextCursor: 1, hasMore: true })
    );
    const incompleteCompletion = scanPlan(
      [candidate("thread-b")],
      progress({ phase: "complete", processed: 2, total: 3, cursor: 1, nextCursor: 2, hasMore: false })
    );

    expect(() => mergeHiddenScanPage(current, emptyPageWithMore)).toThrow("扫描游标未正确推进");
    expect(() => mergeHiddenScanPage(current, incompleteCompletion)).toThrow("扫描游标未正确推进");
  });

  test("stopping before the initial response hides its late candidates and never starts a poll", async () => {
    const firstResponse = deferred<HiddenThreadDeletePlan>();
    const send = vi.fn((request: HiddenScanRequest): Promise<HiddenThreadDeletePlan> => {
      if (request?.mode === "start") return firstResponse.promise;
      if (request?.mode === "cancel") return Promise.resolve(cancelResponse());
      return Promise.reject(new Error("A stopped scan must not poll"));
    });
    const update = vi.fn();
    const runner = new HiddenScanRunner(send, update);
    const running = runner.run();

    await runner.stop();
    expect(send).toHaveBeenCalledExactlyOnceWith({ mode: "start", limit: 100 });

    firstResponse.resolve(scanPlan(
      [candidate("late-thread")],
      progress({ processed: 1, nextCursor: 1 })
    ));
    const result = await running;

    expect(result.scan?.phase).toBe("stopped");
    expect(result.candidates).toEqual([]);
    expect(result.hidden_ids).toEqual([]);
    expect(send).toHaveBeenCalledTimes(2);
    expect(send).toHaveBeenNthCalledWith(2, { mode: "cancel", scanId: "scan-fixture", cursor: 0, limit: 100 });
    expect(send.mock.calls.map(([request]) => request?.mode)).not.toContain("poll");
    expect(update).toHaveBeenCalledExactlyOnceWith(result);
  });

  test("stopping during a poll keeps previously displayed candidates and hides the late page", async () => {
    const first = scanPlan([candidate("visible-thread")], progress({ processed: 1, nextCursor: 1 }));
    const latePage = deferred<HiddenThreadDeletePlan>();
    const pollStarted = deferred<void>();
    const send = vi.fn((request: HiddenScanRequest): Promise<HiddenThreadDeletePlan> => {
      if (request?.mode === "start") return Promise.resolve(first);
      if (request?.mode === "poll") {
        pollStarted.resolve();
        return latePage.promise;
      }
      if (request?.mode === "cancel") return Promise.resolve(cancelResponse());
      return Promise.reject(new Error("A stopped scan must not issue another request"));
    });
    const update = vi.fn();
    const runner = new HiddenScanRunner(send, update);
    const running = runner.run();

    await pollStarted.promise;
    await runner.stop();
    latePage.resolve(scanPlan(
      [candidate("not-yet-visible-thread")],
      progress({ phase: "complete", processed: 2, cursor: 1, nextCursor: 2, hasMore: false })
    ));
    const result = await running;

    expect(result.scan?.phase).toBe("stopped");
    expect(result.candidates.map(item => item.id)).toEqual(["visible-thread"]);
    expect(result.hidden_ids).toEqual(["visible-thread"]);
    expect(send).toHaveBeenCalledTimes(3);
    expect(send).toHaveBeenNthCalledWith(3, { mode: "cancel", scanId: "scan-fixture", cursor: 1, limit: 100 });
    expect(send.mock.calls.map(([request]) => request?.mode)).toEqual(["start", "poll", "cancel"]);
    expect(update).toHaveBeenCalledTimes(2);
    expect(update).toHaveBeenLastCalledWith(result);
    expect(update.mock.calls[0][0].candidates.map((item: HiddenThreadCandidate) => item.id)).toEqual(["visible-thread"]);
  });

});
