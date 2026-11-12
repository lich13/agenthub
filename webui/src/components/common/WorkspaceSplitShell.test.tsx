import { describe, expect, test } from "vitest";
import { providerSessionStatusText } from "./WorkspaceSplitShell";

describe("providerSessionStatusText", () => {
  test("live native states take precedence over the archive flag and timestamp", () => {
    expect(providerSessionStatusText("running", "2026-01-02T03:04:05Z")).toBe("运行中");
    expect(providerSessionStatusText("waiting", "2026-01-02T03:04:05Z")).toBe("等待处理");
    expect(providerSessionStatusText("idle", null, true)).toBe("已归档");
    expect(providerSessionStatusText("unknown")).toBe("状态未知");
    expect(providerSessionStatusText("recent")).toBe("最近");
  });
});
