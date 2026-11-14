import type { SystemCapabilitiesResponse } from "../../types";
import { callCommand } from "./transport";
import { currentDemoFixtureKey, USE_DEMO } from "./shared";

export async function getSystemCapabilities(): Promise<SystemCapabilitiesResponse> {
  if (USE_DEMO) {
    return (await import("./demo")).demoSystemCapabilities(currentDemoFixtureKey());
  }
  return callCommand<SystemCapabilitiesResponse>("system.capabilities");
}
