import { CircleCheck, RotateCw, TriangleAlert, X } from "lucide-react";
import { Component, useEffect, useState, type ErrorInfo, type ReactNode } from "react";

import { CommandPalette } from "./components/CommandPalette";
import { Sidebar } from "./components/Sidebar";
import { Button, IconButton } from "./components/ui";
import { HistoryProvider } from "./lib/history";
import { NavProvider, useNav, useRevealFocus } from "./lib/nav";
import { StudioProvider, useStudio } from "./lib/studio";
import { Bars } from "./views/Bars";
import { Desktop } from "./views/Desktop";
import { Settings } from "./views/Settings";
import { Themes } from "./views/Themes";
import { Widgets } from "./views/Widgets";
import { Windows } from "./views/Windows";

export function App() {
  return (
    <ErrorBoundary>
      <HistoryProvider>
        <StudioProvider>
          <NavProvider>
            <Shell />
          </NavProvider>
        </StudioProvider>
      </HistoryProvider>
    </ErrorBoundary>
  );
}

function Shell() {
  const { route, visit } = useNav();
  const [searching, setSearching] = useState(false);
  useRevealFocus();

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setSearching(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="flex h-full bg-sidebar">
      <Sidebar onSearch={() => setSearching(true)} />
      <main
        // Re-keyed per visit, so each page starts fresh (and fades in) from its route.
        key={visit}
        className="flex min-w-0 flex-1 animate-fade-in flex-col overflow-hidden rounded-tl-xl border-t border-l border-line-strong bg-canvas [--gutter:1.25rem] lg:[--gutter:1.75rem] 2xl:[--gutter:2.5rem]"
      >
        <ErrorBoundary inline>
          {route.page === "desktop" && <Desktop />}
          {route.page === "themes" && <Themes />}
          {route.page === "widgets" && <Widgets />}
          {route.page === "bars" && <Bars />}
          {route.page === "windows" && <Windows />}
          {route.page === "settings" && <Settings />}
        </ErrorBoundary>
      </main>
      <CommandPalette open={searching} onClose={() => setSearching(false)} />
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
          className="selectable pointer-events-auto flex max-w-xl animate-rise items-center gap-2.5 rounded-xl bg-surface-hi py-1.5 pr-1.5 pl-3.5 text-[13px] text-ink shadow-pop ring-1 ring-line-hi"
        >
          {toast.tone === "error" ? <TriangleAlert className="size-4 shrink-0" aria-hidden /> : <CircleCheck className="size-4 shrink-0 text-ink-2" aria-hidden />}
          <span className="py-1 leading-relaxed">{toast.message}</span>
          {toast.action && (
            <Button
              size="sm"
              onClick={() => {
                toast.action?.run();
                dismissToast();
              }}
            >
              {toast.action.label}
            </Button>
          )}
          <IconButton icon={X} label="Dismiss" size="sm" variant="ghost" onClick={dismissToast} />
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
          <p className="font-display text-[15px] font-semibold text-ink">Something went wrong</p>
          <p className="selectable mt-1 text-[13px] text-ink-3">{error.message}</p>
          <Button variant="primary" icon={RotateCw} className="mt-5" onClick={() => this.setState({ error: null })}>
            Try again
          </Button>
        </div>
      </div>
    );
  }
}
