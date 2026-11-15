import { QueryClient } from "@tanstack/react-query";
import { beforeEach, expect, test, vi } from "vitest";
import { getProbeStatus } from "../api/probe";
import { probeStatusQueryKey, refreshProbeStatus } from "./probeStatus";

const state = vi.hoisted(() => ({ revision: 1 }));
vi.mock("./connection", () => ({ connectionSnapshot: () => ({ revision: state.revision }) }));
vi.mock("../api/probe", () => ({ getProbeStatus: vi.fn() }));
beforeEach(() => { vi.clearAllMocks(); state.revision = 1; });

function client() { return new QueryClient({ defaultOptions: { queries: { retry: false } } }); }

test("explicit refresh bypasses fresh client data and replaces the status snapshot", async () => {
  const qc = client();
  qc.setQueryData(probeStatusQueryKey, { available: true, data: { enabled: false } });
  const fresh = { available: true, data: { enabled: true } } as Awaited<ReturnType<typeof getProbeStatus>>;
  vi.mocked(getProbeStatus).mockResolvedValue(fresh);
  await refreshProbeStatus(qc);
  expect(getProbeStatus).toHaveBeenCalledExactlyOnceWith({ refresh: true });
  expect(qc.getQueryData(probeStatusQueryKey)).toEqual(fresh);
  qc.clear();
});

test("a failed explicit refresh exposes the query error and retains the last snapshot", async () => {
  const qc = client();
  const previous = { available: true, data: { enabled: false } };
  qc.setQueryData(probeStatusQueryKey, previous);
  vi.mocked(getProbeStatus).mockRejectedValue(new Error("Connection lost"));
  await refreshProbeStatus(qc);
  expect(qc.getQueryData(probeStatusQueryKey)).toEqual(previous);
  expect(qc.getQueryState(probeStatusQueryKey)?.error?.message).toBe("Connection lost");
  qc.clear();
});

test("machine switching while cancellation is pending does not launch a refresh on the new target", async () => {
  const qc = client();
  vi.spyOn(qc, "cancelQueries").mockImplementation(async () => { state.revision++; });
  await refreshProbeStatus(qc);
  expect(getProbeStatus).not.toHaveBeenCalled();
  qc.clear();
});

test("clearing a machine client cancels its in-flight explicit refresh", async () => {
  const previous = client(); const next = client();
  let resolve!: (value: Awaited<ReturnType<typeof getProbeStatus>>) => void;
  vi.mocked(getProbeStatus).mockReturnValue(new Promise(done => { resolve = done; }));
  const pending = refreshProbeStatus(previous);
  await Promise.resolve();
  state.revision++; await previous.cancelQueries(); previous.clear();
  next.setQueryData(probeStatusQueryKey, { machine: "next" });
  resolve({ available: false }); await pending;
  expect(previous.getQueryData(probeStatusQueryKey)).toBeUndefined();
  expect(next.getQueryData(probeStatusQueryKey)).toEqual({ machine: "next" });
  next.clear();
});
