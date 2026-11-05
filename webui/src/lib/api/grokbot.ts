import type { ProviderDetailRequest, ProviderListRequest, ProviderSessionDetail, ProviderSessionSummary } from "../../types";
import { callCommand } from "./transport";

export function listGrokBotSessions(request: ProviderListRequest = {}): Promise<ProviderSessionSummary[]> {
  return callCommand("grokbot.list", { request });
}
export function getGrokBotSession(request: ProviderDetailRequest): Promise<ProviderSessionDetail> {
  return callCommand("grokbot.detail", { request });
}
