import { expect, test, vi } from "vitest";
import { useBackgroundJobs } from "./jobs";
import { getJob } from "../api/jobs";

const state = vi.hoisted(() => ({ queries: [] as Array<{ queryFn: () => Promise<unknown> }>, invalidate: vi.fn() }));
vi.mock("react", () => ({ useEffect: vi.fn() }));
vi.mock("../api/jobs", () => ({ getJob: vi.fn() }));
vi.mock("@tanstack/react-query", () => ({
  useMutationState: () => ["fixture-job"],
  useQueryClient: () => ({ invalidateQueries: state.invalidate }),
  useQueries: ({ queries }: { queries: typeof state.queries }) => { state.queries = queries; return []; },
  useQuery: vi.fn(),
}));

test("terminal background jobs request only the summary and refresh affected panels", async () => {
  vi.mocked(getJob).mockResolvedValue({ id: "fixture-job", status: "succeeded", output: "" } as Awaited<ReturnType<typeof getJob>>);
  useBackgroundJobs(true);
  await state.queries[0].queryFn();
  expect(getJob).toHaveBeenCalledExactlyOnceWith("fixture-job", false);
  expect(state.invalidate.mock.calls.map(([input]) => input.queryKey)).toEqual([["jobs"], ["update-status"], ["probe-status"], ["probe-events"]]);
});
