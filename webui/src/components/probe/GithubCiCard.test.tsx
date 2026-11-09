import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, test, vi } from "vitest";
import {
  GithubCiCard,
  addRepositorySelection,
  buildRepositoryCatalog,
  ciRunStatus,
  toggleRepositorySelection,
  visibleCiRuns,
} from "./GithubCiCard";
import { demoGithubCiStatus } from "../../lib/api/demo";
import { buildProbeSettingsDraft } from "../../lib/probeUi";

test("CI card shows account, switch states, repositories, statuses and direct workflow links", () => {
  const client = new QueryClient();
  client.setQueryData(["github-ci"], demoGithubCiStatus());
  const draft = buildProbeSettingsDraft({});
  const markup = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <GithubCiCard draft={draft} setDraft={vi.fn()}
        saving={false} saveStatus={null} onSave={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(markup).toContain("@octocat");
  expect(markup).toContain("CI 监控通知");
  expect(markup).toContain("过滤 Bot");
  expect(markup).toContain("已关闭");
  expect(markup).toContain("已开启");
  expect(markup).toContain("监控全部仓库");
  expect(markup).toContain("全部仓库");
  expect(markup).toContain("octocat/AgentHub");
  expect(markup).toContain("octocat/desktop-workflow-integration");
  expect(markup).toContain("运行中");
  expect(markup).toContain("失败");
  expect(markup).toContain("超时");
  expect(markup).toContain("已取消");
  expect(markup).toContain('href="https://github.com/octocat/AgentHub/actions/runs/1"');
  expect(markup).not.toContain('checked=""');
  expect(markup).not.toContain("<small");
});
test("CI card reflects subset repository selection and bot badge when filterBots is off", () => {
  const client = new QueryClient();
  const status = demoGithubCiStatus();
  status.runs[0] = { ...status.runs[0], is_bot: true };
  client.setQueryData(["github-ci"], status);
  const draft = buildProbeSettingsDraft({
    notifications: {
      notify_github_ci: true,
      github_ci_filter_bots: false,
      github_ci_repositories: ["octocat/AgentHub"],
    } as any,
  });
  const markup = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <GithubCiCard draft={draft} setDraft={vi.fn()} saving={false} saveStatus={null} onSave={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(markup).toContain("只监控 1 个仓库");
  expect(markup).toContain("4 次运行");
  expect(markup).toContain("Bot");

  const blankDraft = buildProbeSettingsDraft({
    notifications: {
      notify_github_ci: true,
      github_ci_filter_bots: true,
      github_ci_repositories: ["   "],
    } as any,
  });
  const blankMarkup = renderToStaticMarkup(
    <QueryClientProvider client={client}>
      <GithubCiCard draft={blankDraft} setDraft={vi.fn()} saving={false} saveStatus={null} onSave={vi.fn()} />
    </QueryClientProvider>,
  );
  expect(blankMarkup).toContain("监控全部仓库");
  expect(blankMarkup).not.toContain("只监控 1 个仓库");
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
test("CI shows at most twenty runs after single or multi-repository filtering", () => {
  const sample = demoGithubCiStatus().runs[0];
  const runs = Array.from({ length: 60 }, (_, index) => ({
    ...sample,
    id: index,
    repository_id: Math.floor(index / 10) + 1,
  }));
  expect(visibleCiRuns(runs, "").map(run => run.id)).toEqual(Array.from({ length: 20 }, (_, i) => i));
  expect(visibleCiRuns(runs, "4").map(run => run.id)).toEqual(Array.from({ length: 10 }, (_, i) => i + 30));
  expect(visibleCiRuns([], "")).toEqual([]);
  const withBots = [
    { ...sample, id: 101, repository_id: 1, repository_name: "octocat/AgentHub", is_bot: true },
    { ...sample, id: 102, repository_id: 1, repository_name: "octocat/AgentHub", is_bot: false },
    { ...sample, id: 103, repository_id: 2, repository_name: "octocat/other", is_bot: false },
    { ...sample, id: 104, repository_id: 3, repository_name: "octocat/third", is_bot: false },
  ];
  expect(visibleCiRuns(withBots, "OCTOCAT/agenthub", true).map(run => run.id)).toEqual([102]);
  expect(visibleCiRuns(withBots, "octocat/AgentHub", false).map(run => run.id)).toEqual([101, 102]);
  expect(visibleCiRuns(withBots, ["octocat/AgentHub", "octocat/other"], true).map(run => run.id)).toEqual([102, 103]);
  expect(visibleCiRuns(withBots, ["1", "3"], false).map(run => run.id)).toEqual([101, 102, 104]);

  const catalog = buildRepositoryCatalog(
    [{ id: 1, full_name: "octocat/AgentHub", archived: false }, { id: 9, full_name: "octocat/archived", archived: true }],
    withBots,
    ["custom/manual-repo"],
  );
  expect(catalog.map(item => item.fullName)).toEqual([
    "octocat/AgentHub",
    "octocat/other",
    "octocat/third",
    "custom/manual-repo",
  ]);
  expect(toggleRepositorySelection(catalog[0], [])).toEqual(["octocat/AgentHub"]);
  expect(toggleRepositorySelection(catalog[1], ["octocat/AgentHub"])).toEqual(["octocat/AgentHub", "octocat/other"]);
  expect(toggleRepositorySelection(catalog[0], ["OCTOCAT/agenthub", "octocat/other"])).toEqual(["octocat/other"]);
  expect(addRepositorySelection("agenthub", catalog, ["  "])).toEqual(["octocat/AgentHub"]);
  expect(addRepositorySelection("new-org/new-repo", catalog, ["octocat/AgentHub"])).toEqual([
    "octocat/AgentHub",
    "new-org/new-repo",
  ]);
});
