import {
  Activity,
  Menu,
  PanelLeftClose,
  PanelLeftOpen,
  SlidersHorizontal,
  TriangleAlert
} from "lucide-react";
import { Component, ErrorInfo, Fragment, ReactNode, Suspense, lazy, useState } from "react";
import { AgentBrandIcon } from "./components/common/AgentBrandIcon";
import { MachineButton, RemoteConnectionSettings } from "./components/connection/RemoteConnection";
import { useConnection } from "./lib/query/connection";
import { PROBE_NAV_LABEL } from "./lib/probeUi";
import { SearchWorkspaceProvider } from "./components/common/SessionSearch";
import { useBackgroundJobs } from "./lib/query/jobs";
import {
  useBootstrapRuntimeCapabilities,
  useRuntimeCapabilities,
  useSystemCapabilitiesQuery,
  type RuntimeCapabilityMatrix
} from "./lib/query/system";
import { type RuntimeCapabilityInput } from "./lib/domain/runtimeViewModel";
import {
  navigationLabelsForRuntime as navigationLabelsForRuntimeDomain,
  visibleNavigationItems,
  type View
} from "./lib/domain/codexViewModel";
const ChatWorkspace = lazy(() => import("./components/chat/ChatWorkspace").then(module => ({ default: module.ChatWorkspace })));
const OpsWorkspace = lazy(() => import("./components/ops/OpsWorkspace").then(module => ({ default: module.OpsWorkspace })));
const ProbeWorkspace = lazy(() => import("./components/probe/ProbeWorkspace").then(module => ({ default: module.ProbeWorkspace })));
const GrokWorkspace = lazy(() => import("./components/grok/GrokWorkspace").then(module => ({ default: module.GrokWorkspace })));
const ClaudeWorkspace = lazy(() => import("./components/claude/ClaudeWorkspace").then(module => ({ default: module.ClaudeWorkspace })));
const AntigravityWorkspace = lazy(() => import("./components/providers/ArchivableProviderWorkspace").then(module => ({ default: module.AntigravityWorkspace })));
const DshWorkspace = lazy(() => import("./components/providers/ArchivableProviderWorkspace").then(module => ({ default: module.DshWorkspace })));
export const navigationItems: Array<{ id: View; label: string; icon: ReactNode }> = [
  { id: "codex", label: "Codex", icon: <AgentBrandIcon agent="codex" /> },
  { id: "claude", label: "Claude Code", icon: <AgentBrandIcon agent="claude" /> },
  { id: "grok", label: "Grok Build", icon: <AgentBrandIcon agent="grok" /> },
  { id: "antigravity", label: "Antigravity", icon: <AgentBrandIcon agent="antigravity" /> },
  { id: "dsh", label: "DeepSeek Harness", icon: <AgentBrandIcon agent="dsh" /> },
  { id: "probe", label: PROBE_NAV_LABEL, icon: <Activity /> },
  { id: "ops", label: "设置", icon: <SlidersHorizontal /> }
];

function navigationItemsForCapabilities(capabilities: RuntimeCapabilityMatrix) {
  return visibleNavigationItems(navigationItems, capabilities);
}

export function navigationLabelsForRuntime(desktop?: RuntimeCapabilityInput): string[] {
  return navigationLabelsForRuntimeDomain(navigationItems, desktop);
}

// Navigation preferences contain no machine data. Business views remount per revision.
let lastView: View = "codex";
let lastSettingsSection: "maintenance" | "remote" = "maintenance";

export default function App() {
  const bootstrapCapabilities = useBootstrapRuntimeCapabilities();
  const connection = useConnection();
  const systemCapabilities = useSystemCapabilitiesQuery();
  useBackgroundJobs(true);
  const capabilities = useRuntimeCapabilities(systemCapabilities.data, bootstrapCapabilities);
  const [view, setViewState] = useState<View>(lastView);
  const setView = (next: View) => { lastView = next; setViewState(next); };
  const [settingsSection, setSettingsSectionState] = useState<"maintenance" | "remote">(lastSettingsSection);
  const setSettingsSection = (next: "maintenance" | "remote") => { lastSettingsSection = next; setSettingsSectionState(next); };
  const [mobileThreadsOpen, setMobileThreadsOpen] = useState(true);
  const [navCollapsed, setNavCollapsed] = useState(() => localStorage.getItem("agenthub.nav-collapsed") === "1");

  const toggleNavCollapsed = () => {
    setNavCollapsed((current) => {
      const next = !current;
      localStorage.setItem("agenthub.nav-collapsed", next ? "1" : "0");
      return next;
    });
  };

  return <SearchWorkspaceProvider><div className={`app-shell ${navCollapsed ? "nav-collapsed" : ""}`} data-workspace={view}>
      <SideNav view={view} setView={setView} collapsed={navCollapsed} capabilities={capabilities} onCollapse={toggleNavCollapsed} onConfigure={() => { setView("ops"); setSettingsSection("remote"); }} />
        <main className="main-workspace">
          {(connection.error || systemCapabilities.error) && <div role="alert" className="form-error">{connection.error || systemCapabilities.error?.message}</div>}
          <MobileTopBar onOpenThreads={() => setMobileThreadsOpen(true)} view={view} setView={setView} capabilities={capabilities} />
          <WorkspaceErrorBoundary resetKey={view}>
            <Suspense fallback={<div className="muted-row" role="status">正在读取…</div>}>
            {view === "codex" && (
              <ChatWorkspace
                mobileThreadsOpen={mobileThreadsOpen}
                setMobileThreadsOpen={setMobileThreadsOpen}
                setView={setView}
                capabilities={capabilities}
              />
            )}
            {view === "grok" && <GrokWorkspace />}
            {view === "claude" && <ClaudeWorkspace />}
            {view === "antigravity" && <AntigravityWorkspace capabilities={capabilities} />}
            {view === "dsh" && <DshWorkspace capabilities={capabilities} />}
            {view === "probe" && <ProbeWorkspace capabilities={capabilities} />}
            {view === "ops" && <div className="settings-workspace">
              <header className="settings-header"><h1>设置</h1><div className="settings-tabs" role="tablist">
                <button role="tab" aria-selected={settingsSection === "maintenance"} onClick={() => setSettingsSection("maintenance")}>更新与维护</button>
                <button role="tab" aria-selected={settingsSection === "remote"} onClick={() => setSettingsSection("remote")}>远程连接</button>
              </div></header>
              {settingsSection === "remote" ? <RemoteConnectionSettings /> : <OpsWorkspace capabilities={capabilities} />}
            </div>}
            </Suspense>
          </WorkspaceErrorBoundary>
        </main>
      </div></SearchWorkspaceProvider>;
}

class WorkspaceErrorBoundary extends Component<
  { children: ReactNode; resetKey: string },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("AgentHub workspace render failed", error, info.componentStack);
  }

  componentDidUpdate(previous: { resetKey: string }) {
    if (previous.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null });
    }
  }

  render() {
    if (this.state.error) {
      return (
        <section className="panel wide-panel">
          <header><TriangleAlert size={18} /><strong>视图载入失败</strong></header>
          <div className="form-error">{this.state.error.message || "未知错误"}</div>
        </section>
      );
    }
    return this.props.children;
  }
}

function SideNav({ view, setView, collapsed, capabilities, onCollapse, onConfigure }: {
  view: View;
  setView: (view: View) => void;
  collapsed: boolean;
  capabilities: RuntimeCapabilityMatrix;
  onCollapse: () => void;
  onConfigure: () => void;
}) {
  const items = navigationItemsForCapabilities(capabilities);
  return (
    <aside className="side-nav">
      <div className="nav-brand">
        <svg className="nav-brand-logo" viewBox="0 0 28 28" aria-hidden="true">
          <defs>
            <linearGradient id="agenthub-core-bg" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor="#0E1320" />
              <stop offset="100%" stopColor="#070910" />
            </linearGradient>
            <linearGradient id="agenthub-orbit-a" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor="#38BDF8" />
              <stop offset="100%" stopColor="#818CF8" />
            </linearGradient>
            <linearGradient id="agenthub-orbit-b" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor="#10B981" />
              <stop offset="100%" stopColor="#38BDF8" />
            </linearGradient>
            <linearGradient id="agenthub-orbit-c" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0%" stopColor="#F97316" />
              <stop offset="100%" stopColor="#A855F7" />
            </linearGradient>
          </defs>
          <rect width="28" height="28" rx="7" fill="url(#agenthub-core-bg)" />
          <circle cx="14" cy="14" r="9.8" fill="none" stroke="#94A3B8" strokeOpacity="0.24" strokeWidth="0.75" />
          <ellipse cx="14" cy="14" rx="9.6" ry="4" transform="rotate(-30 14 14)" fill="none" stroke="url(#agenthub-orbit-a)" strokeWidth="1.35" />
          <ellipse cx="14" cy="14" rx="9.6" ry="4" transform="rotate(30 14 14)" fill="none" stroke="url(#agenthub-orbit-b)" strokeWidth="1.35" />
          <ellipse cx="14" cy="14" rx="9.6" ry="4" transform="rotate(90 14 14)" fill="none" stroke="url(#agenthub-orbit-c)" strokeWidth="1.35" />
          <circle cx="14" cy="14" r="3.1" fill="#0F172A" stroke="#38BDF8" strokeWidth="0.9" />
          <path d="M14 11.5L14.9 13.1L16.5 14L14.9 14.9L14 16.5L13.1 14.9L11.5 14L13.1 13.1Z" fill="#F8FAFC" />
        </svg>
        <strong>AgentHub</strong>
        <button className="icon-button nav-collapse-button" onClick={onCollapse} title={collapsed ? "展开导航" : "折叠导航"} aria-expanded={!collapsed}>{collapsed ? <PanelLeftOpen size={17} /> : <PanelLeftClose size={17} />}</button>
      </div>
      <MachineButton collapsed={collapsed} onConfigure={onConfigure} />
      <nav>
        {items.map((item) => (
          <Fragment key={item.id}>
            {item.id === "probe" && <div className="nav-section-divider" aria-hidden="true" />}
            <NavButton navId={item.id} icon={item.icon} active={view === item.id} onClick={() => setView(item.id)}>
              {item.label}
            </NavButton>
          </Fragment>
        ))}
      </nav>
    </aside>
  );
}

function NavButton({ navId, icon, active, onClick, children }: { navId: View; icon: ReactNode; active: boolean; onClick: () => void; children: ReactNode }) {
  return <button data-nav-id={navId} className={`nav-button ${active ? "active" : ""}`} title={String(children)} aria-label={String(children)} aria-current={active ? "page" : undefined} onClick={onClick}><i className="nav-icon-badge" aria-hidden="true">{icon}</i><span>{children}</span></button>;
}

function MobileTopBar({ onOpenThreads, view, setView, capabilities }: {
  onOpenThreads: () => void;
  view: View;
  setView: (view: View) => void;
  capabilities: RuntimeCapabilityMatrix;
}) {
  const items = navigationItemsForCapabilities(capabilities);
  const current = items.find((item) => item.id === view);
  return (
    <>
      <div className="mobile-topbar">
        <button className="icon-button" onClick={onOpenThreads} disabled={view !== "codex"} title={view === "codex" ? "打开线程" : "线程列表仅用于 Codex"}>
          <Menu size={20} />
        </button>
        <span>{current?.label ?? "AgentHub"}</span>
        <div className="topbar-dot" />
      </div>
      <div className="mobile-tabs">
        {items.map((item) => (
          <button key={item.id} data-nav-id={item.id} title={item.label} aria-label={item.label} className={view === item.id ? "active" : ""} onClick={() => setView(item.id)}>
            {item.icon}
            <span>{item.label}</span>
          </button>
        ))}
      </div>
    </>
  );
}
