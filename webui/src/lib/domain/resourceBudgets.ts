/** Per-WebView retained-text budgets. Rendered Markdown elements and React /
 * DOM allocations are separate; these values are not a process memory cap. */
export const RENDER_CACHE_BUDGET = {
  entries: 512,
  instructionsBytes: 8 * 1024 * 1024,
  contextBytes: 8 * 1024 * 1024,
  markdownBytes: 8 * 1024 * 1024
} as const;
