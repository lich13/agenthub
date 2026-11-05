import { useId } from "react";

export type AgentBrandId = "codex" | "claude" | "grok" | "grokbot" | "antigravity" | "dsh";

// Exact official DeepSeek whale path extracted from DeepSeek Harness.app (@deepseek-ai/dsh-web-frontend/dist/favicon.svg, viewBox 0 0 50 50)
const DEEPSEEK_OFFICIAL_PATH =
  "M 48.84 10.05 C 48.32 9.79 48.1 10.28 47.8 10.53 C 47.7 10.61 47.61 10.71 47.53 10.81 C 46.78 11.62 45.9 12.16 44.76 12.1 C 43.09 12 41.67 12.54 40.41 13.84 C 40.14 12.23 39.25 11.27 37.89 10.66 C 37.18 10.34 36.47 10.02 35.97 9.32 C 35.62 8.82 35.53 8.27 35.36 7.73 C 35.25 7.4 35.14 7.06 34.77 7.01 C 34.36 6.94 34.21 7.29 34.05 7.58 C 33.42 8.75 33.17 10.05 33.2 11.36 C 33.25 14.31 34.47 16.66 36.9 18.34 C 37.18 18.53 37.25 18.72 37.16 19 C 36.99 19.58 36.8 20.14 36.62 20.71 C 36.51 21.08 36.35 21.16 35.96 21 C 34.63 20.43 33.48 19.59 32.46 18.58 C 30.74 16.87 29.18 14.99 27.23 13.52 C 26.78 13.18 26.32 12.86 25.85 12.55 C 23.86 10.58 26.11 8.97 26.63 8.78 C 27.17 8.58 26.82 7.89 25.06 7.9 C 23.3 7.9 21.7 8.5 19.65 9.3 C 19.35 9.42 19.03 9.51 18.71 9.58 C 16.85 9.22 14.92 9.14 12.9 9.38 C 9.11 9.81 6.07 11.64 3.84 14.77 C 1.16 18.53 0.53 22.8 1.31 27.26 C 2.12 31.95 4.47 35.84 8.07 38.88 C 11.82 42.03 16.13 43.58 21.04 43.28 C 24.03 43.1 27.35 42.7 31.1 39.46 C 32.05 39.94 33.04 40.13 34.69 40.27 C 35.95 40.39 37.18 40.21 38.12 40.01 C 39.6 39.69 39.5 38.29 38.96 38.03 C 34.62 35.97 35.58 36.81 34.71 36.13 C 36.92 33.46 40.24 30.7 41.54 21.73 C 41.64 21.02 41.56 20.57 41.54 19.99 C 41.53 19.64 41.61 19.5 42 19.46 C 43.09 19.34 44.15 19.03 45.12 18.49 C 47.93 16.92 49.06 14.34 49.33 11.26 C 49.37 10.78 49.32 10.3 48.84 10.05 Z M 24.33 37.84 C 20.12 34.46 18.08 33.35 17.24 33.4 C 16.45 33.45 16.59 34.37 16.76 34.97 C 16.94 35.56 17.18 35.97 17.51 36.49 C 17.74 36.83 17.9 37.34 17.28 37.73 C 15.93 38.58 13.57 37.44 13.46 37.38 C 10.72 35.74 8.43 33.56 6.81 30.58 C 5.25 27.72 4.35 24.65 4.2 21.37 C 4.16 20.58 4.39 20.3 5.16 20.15 C 6.18 19.96 7.22 19.92 8.24 20.07 C 12.53 20.71 16.19 22.67 19.25 25.78 C 21 27.54 22.33 29.66 23.69 31.72 C 25.14 33.91 26.7 36 28.68 37.71 C 29.38 38.31 29.94 38.77 30.48 39.1 C 28.86 39.29 26.17 39.33 24.33 37.84 Z M 26.34 24.6 C 26.34 24.25 26.62 23.97 26.97 23.97 C 27.04 23.97 27.12 23.98 27.18 24.01 C 27.27 24.04 27.34 24.09 27.41 24.16 C 27.52 24.27 27.58 24.43 27.58 24.6 C 27.58 24.95 27.3 25.23 26.96 25.23 C 26.61 25.23 26.34 24.95 26.34 24.6 Z M 32.61 27.88 C 32.2 28.05 31.8 28.19 31.42 28.21 C 30.82 28.24 30.16 27.99 29.81 27.69 C 29.26 27.22 28.86 26.95 28.7 26.13 C 28.63 25.78 28.67 25.23 28.73 24.92 C 28.87 24.25 28.71 23.82 28.25 23.42 C 27.87 23.1 27.39 23.02 26.86 23.02 C 26.67 23.02 26.48 22.93 26.35 22.86 C 26.13 22.74 25.95 22.46 26.12 22.12 C 26.18 22.01 26.45 21.74 26.51 21.69 C 27.23 21.27 28.05 21.41 28.82 21.72 C 29.53 22.02 30.06 22.56 30.83 23.33 C 31.62 24.26 31.76 24.51 32.21 25.21 C 32.57 25.75 32.89 26.31 33.11 26.95 C 33.24 27.35 33.07 27.68 32.61 27.88 Z";

export function AgentBrandIcon({
  agent,
  size = 17,
  className,
}: {
  agent: AgentBrandId;
  size?: number;
  className?: string;
}) {
  const uid = useId().replace(/:/g, "");
  const classes = ["agent-brand-icon", `agent-brand-${agent}`, className]
    .filter(Boolean)
    .join(" ");

  if (agent === "codex") {
    // Official OpenAI 6-fold rotational blossom knot
    return (
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        className={classes}
        fill="currentColor"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M 22.282 9.821 a 5.985 5.985 0 0 0 -0.516 -4.91 6.046 6.046 0 0 0 -6.51 -2.9 A 6.065 6.065 0 0 0 4.981 4.18 a 5.985 5.985 0 0 0 -3.998 2.9 6.046 6.046 0 0 0 0.743 7.097 5.98 5.98 0 0 0 0.51 4.911 6.051 6.051 0 0 0 6.515 2.9 A 5.985 5.985 0 0 0 13.26 24 a 6.056 6.056 0 0 0 5.772 -4.206 5.99 5.99 0 0 0 3.997 -2.9 6.056 6.056 0 0 0 -0.747 -7.073 z M 13.26 22.43 a 4.476 4.476 0 0 1 -2.876 -1.04 l 0.141 -0.081 4.779 -2.758 a 0.795 0.795 0 0 0 0.392 -0.681 v -6.737 l 2.02 1.168 a 0.071 0.071 0 0 1 0.038 0.052 v 5.583 a 4.504 4.504 0 0 1 -4.494 4.494 z M 3.6 18.304 a 4.47 4.47 0 0 1 -0.535 -3.014 l 0.142 0.085 4.783 2.759 a 0.771 0.771 0 0 0 0.78 0 l 5.843 -3.369 v 2.332 a 0.08 0.08 0 0 1 -0.033 0.062 L 9.74 19.95 a 4.5 4.5 0 0 1 -6.14 -1.646 z M 2.34 7.896 a 4.485 4.485 0 0 1 2.366 -1.973 V 11.6 a 0.766 0.766 0 0 0 0.388 0.676 l 5.815 3.355 -2.02 1.168 a 0.076 0.076 0 0 1 -0.071 0 l -4.83 -2.786 A 4.504 4.504 0 0 1 2.34 7.872 z m 16.597 3.855 l -5.833 -3.387 L 15.119 7.2 a 0.076 0.076 0 0 1 0.071 0 l 4.83 2.791 a 4.494 4.494 0 0 1 -0.676 8.105 v -5.678 a 0.79 0.79 0 0 0 -0.407 -0.667 z m 2.01 -3.023 l -0.141 -0.085 -4.774 -2.782 a 0.776 0.776 0 0 0 -0.785 0 L 9.409 9.23 V 6.897 a 0.066 0.066 0 0 1 0.028 -0.061 l 4.83 -2.787 a 4.5 4.5 0 0 1 6.68 4.66 z m -12.64 4.135 l -2.02 -1.164 a 0.08 0.08 0 0 1 -0.038 -0.057 V 6.075 a 4.5 4.5 0 0 1 7.375 -3.453 l -0.142 0.08 L 8.704 5.46 a 0.795 0.795 0 0 0 -0.393 0.681 z m 1.097 -2.365 l 2.602 -1.5 2.607 1.5 v 2.999 l -2.597 1.5 -2.607 -1.5 z" />
      </svg>
    );
  }

  if (agent === "claude") {
    // Official Anthropic Claude starburst spark
    return (
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        className={classes}
        fill="currentColor"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M 4.709 15.955 l 4.72 -2.647 0.08 -0.23 -0.08 -0.128 H 9.2 l -0.79 -0.048 -2.698 -0.073 -2.339 -0.097 -2.266 -0.122 -0.571 -0.121 L 0 11.784 l 0.055 -0.352 0.48 -0.321 0.686 0.06 1.52 0.103 2.278 0.158 1.652 0.097 2.449 0.255 h 0.389 l 0.055 -0.157 -0.134 -0.098 -0.103 -0.097 -2.358 -1.596 -2.552 -1.688 -1.336 -0.972 -0.724 -0.491 -0.364 -0.462 -0.158 -1.008 0.656 -0.722 0.881 0.06 0.225 0.061 0.893 0.686 1.908 1.476 2.491 1.833 0.365 0.304 0.145 -0.103 0.019 -0.073 -0.164 -0.274 -1.355 -2.446 -1.446 -2.49 -0.644 -1.032 -0.17 -0.619 a 2.97 2.97 0 0 1 -0.104 -0.729 L 6.283 0.134 6.696 0 l 0.996 0.134 0.42 0.364 0.62 1.414 1.002 2.229 1.555 3.03 0.456 0.898 0.243 0.832 0.091 0.255 h 0.158 V 9.01 l 0.128 -1.706 0.237 -2.095 0.23 -2.695 0.08 -0.76 0.376 -0.91 0.747 -0.492 0.584 0.28 0.48 0.685 -0.067 0.444 -0.286 1.851 -0.559 2.903 -0.364 1.942 h 0.212 l 0.243 -0.242 0.985 -1.306 1.652 -2.064 0.73 -0.82 0.85 -0.904 0.547 -0.431 h 1.033 l 0.76 1.129 -0.34 1.166 -1.064 1.347 -0.881 1.142 -1.264 1.7 -0.79 1.36 0.073 0.11 0.188 -0.02 2.856 -0.606 1.543 -0.28 1.841 -0.315 0.833 0.388 0.091 0.395 -0.328 0.807 -1.969 0.486 -2.309 0.462 -3.439 0.813 -0.042 0.03 0.049 0.061 1.549 0.146 0.662 0.036 h 1.622 l 3.02 0.225 0.79 0.522 0.474 0.638 -0.079 0.485 -1.215 0.62 -1.64 -0.389 -3.829 -0.91 -1.312 -0.329 h -0.182 v 0.11 l 1.093 1.068 2.006 1.81 2.509 2.33 0.127 0.578 -0.322 0.455 -0.34 -0.049 -2.205 -1.657 -0.851 -0.747 -1.926 -1.62 h -0.128 v 0.17 l 0.444 0.649 2.345 3.521 0.122 1.08 -0.17 0.353 -0.608 0.213 -0.668 -0.122 -1.374 -1.925 -1.415 -2.167 -1.143 -1.943 -0.14 0.08 -0.674 7.254 -0.316 0.37 -0.729 0.28 -0.607 -0.461 -0.322 -0.747 0.322 -1.476 0.389 -1.924 0.315 -1.53 0.286 -1.9 0.17 -0.632 -0.012 -0.042 -0.14 0.018 -1.434 1.967 -2.18 2.945 -1.726 1.845 -0.414 0.164 -0.717 -0.37 0.067 -0.662 0.401 -0.589 2.388 -3.036 1.44 -1.882 0.93 -1.086 -0.006 -0.158 h -0.055 L 4.132 18.56 l -1.13 0.146 -0.487 -0.456 0.061 -0.746 0.231 -0.243 1.908 -1.312 -0.006 0.006 z" />
      </svg>
    );
  }

  if ((agent === "grok" || agent === "grokbot")) {
    // Official Grok Build macOS app icon (Grok Bot.app): dark squircle + 3D silver spherical bot dome + tilted visor capsule eyes
    const clipId = `grok-clip-${uid}`;
    const domeId = `grok-dome-${uid}`;
    const bgId = `grok-bg-${uid}`;
    return (
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        className={classes}
        aria-hidden="true"
        focusable="false"
      >
        <defs>
          <clipPath id={clipId}>
            <rect x="1.6" y="1.6" width="20.8" height="20.8" rx="5.2" />
          </clipPath>
          <linearGradient id={bgId} x1="4" y1="2" x2="20" y2="22" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#2E2E32" />
            <stop offset="100%" stopColor="#1B1B1E" />
          </linearGradient>
          <radialGradient id={domeId} cx="11.4" cy="9.2" r="14.2" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#FFFFFF" />
            <stop offset="48%" stopColor="#ECEEF2" />
            <stop offset="82%" stopColor="#C3C7D0" />
            <stop offset="100%" stopColor="#9DA2AC" />
          </radialGradient>
        </defs>
        <g clipPath={`url(#${clipId})`}>
          <rect x="1.6" y="1.6" width="20.8" height="20.8" fill={`url(#${bgId})`} />
          <circle
            cx="10.55"
            cy="17.85"
            r="12.55"
            fill={`url(#${domeId})`}
            stroke="#FFFFFF"
            strokeWidth="0.5"
          />
          <rect
            x="8.48"
            y="11.7"
            width="6.39"
            height="2.95"
            rx="1.48"
            transform="rotate(54.6 11.67 13.18)"
            fill="#17171A"
            stroke="#F8FAFC"
            strokeWidth="0.34"
          />
          <rect
            x="14"
            y="9.62"
            width="6.39"
            height="1.96"
            rx="0.98"
            transform="rotate(54.3 17.19 10.6)"
            fill="#17171A"
            stroke="#F8FAFC"
            strokeWidth="0.32"
          />
        </g>
        <rect
          x="1.85"
          y="1.85"
          width="20.3"
          height="20.3"
          rx="4.95"
          fill="none"
          stroke="rgba(255, 255, 255, 0.22)"
          strokeWidth="0.5"
        />
      </svg>
    );
  }

  if (agent === "antigravity") {
    // Official Google Antigravity macOS app icon (Antigravity.app): sub-pixel traced 4-color fluid "A" ribbon
    const baseId = `ag-base-${uid}`;
    const greenId = `ag-green-${uid}`;
    const warmId = `ag-warm-${uid}`;
    const clipId = `ag-clip-${uid}`;
    const archPath =
      "M 12 2.76 C 9.38 2.76 7.82 6.84 6.18 12.42 C 5.14 15.96 3.74 18.46 2.48 19.96 C 1.92 20.62 2.48 21.32 3.38 21.1 C 5.12 20.66 6.88 17.88 8.76 14.84 C 9.78 13.18 10.76 12.68 12 12.68 C 13.24 12.68 14.22 13.18 15.24 14.84 C 17.12 17.88 18.88 20.66 20.62 21.1 C 21.52 21.32 22.08 20.62 21.52 19.96 C 20.26 18.46 18.86 15.96 17.82 12.42 C 16.18 6.84 14.62 2.76 12 2.76 Z";
    return (
      <svg
        viewBox="0 0 24 24"
        width={size}
        height={size}
        className={classes}
        aria-hidden="true"
        focusable="false"
      >
        <defs>
          <clipPath id={clipId}>
            <path d={archPath} />
          </clipPath>
          <linearGradient id={baseId} x1="12" y1="21.2" x2="12" y2="2.8" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#428DFC" />
            <stop offset="52%" stopColor="#3B82F6" />
            <stop offset="100%" stopColor="#FBBC04" />
          </linearGradient>
          <radialGradient id={greenId} cx="7.8" cy="7.4" r="7.4" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#4CAF50" />
            <stop offset="54%" stopColor="#8BC34A" stopOpacity="0.86" />
            <stop offset="100%" stopColor="#34A853" stopOpacity="0" />
          </radialGradient>
          <radialGradient id={warmId} cx="14.6" cy="5.8" r="7.8" gradientUnits="userSpaceOnUse">
            <stop offset="0%" stopColor="#F4511E" />
            <stop offset="48%" stopColor="#FB8C00" stopOpacity="0.88" />
            <stop offset="100%" stopColor="#EA4335" stopOpacity="0" />
          </radialGradient>
        </defs>
        <g clipPath={`url(#${clipId})`}>
          <rect x="1.8" y="2.4" width="20.4" height="19.2" fill={`url(#${baseId})`} />
          <circle cx="7.8" cy="7.4" r="7.4" fill={`url(#${greenId})`} />
          <circle cx="14.6" cy="5.8" r="7.8" fill={`url(#${warmId})`} />
        </g>
      </svg>
    );
  }

  // Official DeepSeek leaping whale SVG from DeepSeek Harness.app (@deepseek-ai/dsh-web-frontend/dist/favicon.svg)
  return (
    <svg
      viewBox="0 0 50 50"
      width={size}
      height={size}
      className={classes}
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      <path d={DEEPSEEK_OFFICIAL_PATH} />
    </svg>
  );
}
