import { RotateCw, TriangleAlert, X } from "lucide-react";
import { Component, useState, type ErrorInfo, type ReactNode } from "react";

import { Sidebar, type View } from "./components/Sidebar";
import { Button } from "./components/ui";
import { StudioProvider, useStudio } from "./lib/studio";
import { Bars } from "./views/Bars";
import { Gallery } from "./views/Gallery";
import { NewTheme } from "./views/NewTheme";
import { Settings } from "./views/Settings";
import { Widgets } from "./views/Widgets";

export function App() {
  return (
    <ErrorBoundary>
      <StudioProvider>
        <Shell />
      </StudioProvider>
    </ErrorBoundary>
  );
}

function Shell() {
  const [view, setView] = useState<View>("gallery");
  return (
    <div className="flex h-full bg-sidebar">
      <Sidebar view={view} onNavigate={setView} />
      <main
        // Re-key per view so each one fades in.
        key={view}
        className="min-w-0 flex-1 animate-rise overflow-y-auto rounded-tl-2xl border-t border-l border-line bg-canvas px-9 pt-9 pb-8"
      >
        <ErrorBoundary inline>
          {view === "gallery" && <Gallery onNavigate={setView} />}
          {view === "widgets" && <Widgets />}
          {view === "bars" && <Bars />}
          {view === "new" && <NewTheme onNavigate={setView} />}
          {view === "settings" && <Settings />}
        </ErrorBoundary>
      </main>
      <Toaster />
    </div>
  );
}

function Toaster() {
  const { toast, dismissToast } = useStudio();
  return (
    <div aria-live="polite" className="pointer-events-none fixed inset-x-0 bottom-6 z-50 flex justify-center px-6">
      {toast && (
        <div
          key={toast.id}
          role={toast.tone === "error" ? "alert" : "status"}
          className="selectable pointer-events-auto flex max-w-xl animate-rise items-start gap-3 rounded-xl bg-surface-hi py-2.5 pr-2 pl-4 text-[13px] text-ink shadow-card ring-1 ring-line-hi"
        >
          {toast.tone === "error" && <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />}
          <span className="py-0.5">{toast.message}</span>
          <button
            type="button"
            onClick={dismissToast}
            aria-label="Dismiss"
            className="grid size-6 shrink-0 cursor-default place-items-center rounded-md text-ink-3 transition-colors hover:bg-line hover:text-ink"
          >
            <X className="size-3.5" aria-hidden />
          </button>
        </div>
      )}
    </div>
  );
}

type BoundaryProps = { children: ReactNode; /** Fill only the content area, not the window. */ inline?: boolean };

/** A render error shows a message and a retry instead of a blank window. */
class ErrorBoundary extends Component<BoundaryProps, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className={`grid place-items-center text-center ${this.props.inline ? "h-full" : "h-full bg-canvas"}`}>
        <div className="max-w-md">
          <span className="mx-auto mb-4 grid size-12 place-items-center rounded-2xl bg-surface-hi text-ink-2">
            <TriangleAlert className="size-6" strokeWidth={1.5} aria-hidden />
          </span>
          <p className="font-display text-lg font-semibold text-ink">Something went wrong</p>
          <p className="selectable mt-1 text-[13px] text-ink-3">{error.message}</p>
          <Button variant="primary" icon={RotateCw} className="mt-5" onClick={() => this.setState({ error: null })}>
            Try again
          </Button>
        </div>
      </div>
    );
  }
}
