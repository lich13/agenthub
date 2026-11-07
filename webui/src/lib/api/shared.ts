import type { OptionalResult } from "../../types";
import { RuntimeUnavailableError } from "./transport";
import type { DemoFixtureKey } from "../domain/demoCore";

export const USE_DEMO = import.meta.env.DEV && import.meta.env.VITE_USE_REAL_API !== "1";

let demoFixtureKey: DemoFixtureKey = "macos-tauri";

export function configureDemoFixtureKey(fixture: DemoFixtureKey): void {
  demoFixtureKey = fixture;
}

export function currentDemoFixtureKey(): DemoFixtureKey {
  return demoFixtureKey;
}

export class ApiError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
    this.name = "ApiError";
  }
}

export function objectValue(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function snakeCaseKey(key: string): string {
  return key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

function snakeCaseKeys(value: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, item] of Object.entries(value)) {
    out[snakeCaseKey(key)] = item;
  }
  return out;
}

export function normalizeProbeRuntimePayload(value: unknown): Record<string, unknown> {
  const raw = objectValue(value);
  const top = snakeCaseKeys(raw);
  const codex = snakeCaseKeys(objectValue(raw.codex));
  const probe = snakeCaseKeys(objectValue(raw.probe));
  const notifications = snakeCaseKeys(objectValue(raw.notifications));
  const nestedNotifications = snakeCaseKeys(objectValue(probe.notifications));
  const nestedObservability = snakeCaseKeys(objectValue(probe.observability));
  const hooks = snakeCaseKeys(objectValue(probe.hooks));
  return {
    ...top,
    codex,
    probe: {
      ...probe,
      hooks,
      notifications: nestedNotifications,
      observability: nestedObservability,
    },
    notifications,
  };
}

export function normalizeOptionalResult<T>(payload: unknown): OptionalResult<T> {
  if (payload && typeof payload === "object" && "available" in payload && ("data" in payload || "error" in payload || "reason" in payload)) {
    const wrapped = payload as { available?: unknown; data?: T; reason?: unknown; error?: unknown };
    if (wrapped.available === false) {
      return {
        available: false,
        reason: typeof wrapped.reason === "string" ? wrapped.reason : null,
        error: typeof wrapped.error === "string" ? wrapped.error : undefined
      };
    }
    return {
      available: true,
      data: wrapped.data as T
    };
  }
  return { available: true, data: payload as T };
}

export function jobIdFromRuntimeResult(result: { job_id?: string | null; jobId?: string | null }, fallback: string): { job_id: string } {
  return { job_id: result.job_id ?? result.jobId ?? fallback };
}
