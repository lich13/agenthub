import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test, vi } from "vitest";
import { GithubCiCard, ciRunStatus, visibleCiRuns } from "./GithubCiCard";
import { demoGithubCiStatus } from "../../lib/api/demo";
import { buildProbeSettingsDraft } from "../../lib/probeUi";

test("CI card shows account, repositories, statuses and direct workflow links", () => {
  const client = new QueryClient();
  client.setQueryData(["github-ci"], demoGithubCiStatus());
  const markup = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <GithubCiCard draft={buildProbeSettingsDraft({})} setDraft={vi.fn()}
        saving={false} saveStatus={null} onSave={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(markup).toContain("@octocat");
  expect(markup).toContain("全部仓库");
  expect(markup).toContain("运行中");
  expect(markup).toContain("失败");
  expect(markup).toContain("超时");
  expect(markup).toContain("已取消");
  expect(markup).toContain('href="https://github.com/octocat/AgentHub/actions/runs/1"');
  expect(markup).not.toContain('checked=""');
  expect(markup).not.toContain("<small");
});
test("CI error and empty states remain visible", () => {
  const client = new QueryClient();
  client.setQueryData(["github-ci"], { ...demoGithubCiStatus(), account: null, runs: [], error: "当前用户尚未登录 gh" });
  const markup = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <GithubCiCard draft={null} setDraft={vi.fn()} saving={false} saveStatus={null} onSave={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(markup).toContain('role="alert"');
  expect(markup).toContain("当前用户尚未登录 gh");
  expect(markup).toContain("暂无工作流运行");
});
test("nonterminal and cancelled runs keep distinct status labels", () => {
  const run = demoGithubCiStatus().runs[0];
  expect(ciRunStatus({ ...run, status: "queued", conclusion: null }).label).toBe("排队中");
  expect(ciRunStatus({ ...run, conclusion: "cancelled" }).tone).toBe("neutral");
});
test("CI shows at most twenty runs after repository filtering", () => {
  const sample = demoGithubCiStatus().runs[0];
  const runs = Array.from({ length: 60 }, (_, index) => ({
    ...sample,
    id: index,
    repository_id: Math.floor(index / 10) + 1,
  }));
  expect(visibleCiRuns(runs, "").map(run => run.id)).toEqual(Array.from({ length: 20 }, (_, i) => i));
  expect(visibleCiRuns(runs, "4").map(run => run.id)).toEqual(Array.from({ length: 10 }, (_, i) => i + 30));
  expect(visibleCiRuns([], "")).toEqual([]);
});
