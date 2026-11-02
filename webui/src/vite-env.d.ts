/// <reference types="vite/client" />

declare var __AGENTHUB_DESKTOP_RUNTIME__: boolean | undefined;
declare var __AGENTHUB_TEST_INVOKE__:
  | ((command: string, args?: Record<string, unknown>) => unknown)
  | undefined;

interface Window {
  __AGENTHUB_DESKTOP_RUNTIME__?: boolean;
}
