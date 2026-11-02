import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import React, { Component, ErrorInfo, ReactNode, useEffect, useMemo } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
import { initializeConnection, useConnection } from "./lib/runtime";
import { clearAttachmentCache } from "./lib/query/attachments";

declare global {
  interface Window {
    __AGENTHUB_BOOT__?: {
      mounted: boolean;
      showError: (error: unknown) => void;
    };
  }
}

function MachineWorkspace() {
  const connection = useConnection();
  const queryClient = useMemo(() => new QueryClient({ defaultOptions: {
    queries: { retry: 1 }, mutations: { retry: false }
  } }), [connection.revision]);
  useEffect(() => { void initializeConnection(); }, []);
  useEffect(() => () => { void queryClient.cancelQueries(); queryClient.clear(); clearAttachmentCache(); }, [queryClient]);
  if (!connection.ready) return null;
  return <QueryClientProvider client={queryClient}><App key={connection.revision} /></QueryClientProvider>;
}

class RootErrorBoundary extends Component<
  { children: ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    try {
      localStorage.setItem("agenthub.lastRenderError", error.message);
    } catch {
      // ignore storage failures
    }
    console.error("AgentHub render failed", error, info.componentStack);
  }

  render() {
    if (!this.state.error) {
      return this.props.children;
    }
    return (
      <main className="fatal-screen">
        <section className="panel wide-panel">
          <header><strong>界面载入失败</strong></header>
          <div className="form-error">{this.state.error.message || "未知错误"}</div>
        </section>
      </main>
    );
  }
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <RootErrorBoundary><MachineWorkspace /></RootErrorBoundary>
  </React.StrictMode>
);
if (window.__AGENTHUB_BOOT__) {
  window.__AGENTHUB_BOOT__.mounted = true;
}
