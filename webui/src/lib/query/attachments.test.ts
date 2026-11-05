import { afterEach, expect, test, vi } from "vitest";
import { machineScope } from "../runtime";
import { readSessionAttachment } from "../api/sessions";
import { clearAttachmentCache, readAttachment } from "./attachments";

vi.mock("../runtime", () => ({ machineScope: vi.fn(() => "local:") }));
vi.mock("../api/sessions", () => ({ readSessionAttachment: vi.fn() }));
const request = { provider: "codex" as const, sessionKey: "thread", messageId: "message", attachmentId: "attachment" };
afterEach(() => { clearAttachmentCache(); vi.resetAllMocks(); vi.mocked(machineScope).mockReturnValue("local:"); });

test("concurrent reads and repeated polling reuse a single response", async () => {
  vi.mocked(readSessionAttachment).mockResolvedValue({ mimeType: "image/png", base64: "aGVsbG8=" });
  const [a, b] = await Promise.all([readAttachment(request), readAttachment(request)]);
  expect(a).toBe(b); expect(await readAttachment(request)).toBe(a);
  expect(readSessionAttachment).toHaveBeenCalledTimes(1);
});

test("a response arriving after a connection change cannot repopulate the cache", async () => {
  let resolve!: (value: { mimeType: string; base64: string }) => void;
  vi.mocked(readSessionAttachment).mockImplementationOnce(() => new Promise(done => { resolve = done; }));
  const pending = readAttachment(request); clearAttachmentCache();
  resolve({ mimeType: "image/png", base64: "aGVsbG8=" }); await expect(pending).rejects.toThrow("已丢弃旧附件");
  vi.mocked(readSessionAttachment).mockResolvedValue({ mimeType: "image/png", base64: "bmV3" });
  expect(await readAttachment(request)).toContain("bmV3"); expect(readSessionAttachment).toHaveBeenCalledTimes(2);
});

test("large previews evict older entries and failed reads remain retryable", async () => {
  vi.mocked(readSessionAttachment).mockRejectedValueOnce(new Error("unavailable"));
  await expect(readAttachment(request)).rejects.toThrow("unavailable");
  vi.mocked(readSessionAttachment).mockResolvedValue({ mimeType: "image/png", base64: "A".repeat(11 * 1024 * 1024) });
  for (let index = 0; index < 3; index++) await readAttachment({ ...request, attachmentId: String(index) });
  await readAttachment({ ...request, attachmentId: "0" });
  expect(readSessionAttachment).toHaveBeenCalledTimes(5);
});


test("identical attachment identities on local and remote machines never share cached bytes", async () => {
  vi.mocked(readSessionAttachment).mockResolvedValueOnce({ mimeType: "image/png", base64: "bG9jYWw=" }).mockResolvedValueOnce({ mimeType: "image/png", base64: "cmVtb3Rl" });
  expect(await readAttachment(request)).toContain("bG9jYWw=");
  vi.mocked(machineScope).mockReturnValue("remote:fixture-host");
  expect(await readAttachment(request)).toContain("cmVtb3Rl");
  expect(readSessionAttachment).toHaveBeenCalledTimes(2);
});

test("subagent reads preserve root authorization identity and cannot reuse bytes from another root", async () => {
  vi.mocked(readSessionAttachment)
    .mockResolvedValueOnce({ mimeType: "image/png", base64: "cm9vdC1h" })
    .mockResolvedValueOnce({ mimeType: "image/png", base64: "cm9vdC1i" })
    .mockResolvedValueOnce({ mimeType: "image/png", base64: "ZGlyZWN0" });
  const child = { ...request, sessionKey: "child-fixture", rootThreadId: "root-a" };
  const first = await readAttachment(child);
  expect(await readAttachment(child)).toBe(first);
  expect(first).toContain("cm9vdC1h");
  expect(await readAttachment({ ...child, rootThreadId: "root-b" })).toContain("cm9vdC1i");
  expect(await readAttachment({ ...request, sessionKey: "child-fixture" })).toContain("ZGlyZWN0");
  expect(readSessionAttachment).toHaveBeenNthCalledWith(1, child);
  expect(readSessionAttachment).toHaveBeenNthCalledWith(2, { ...child, rootThreadId: "root-b" });
  expect(readSessionAttachment).toHaveBeenNthCalledWith(3, { ...request, sessionKey: "child-fixture" });
  expect(readSessionAttachment).toHaveBeenCalledTimes(3);
});
