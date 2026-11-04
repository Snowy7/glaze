// Where Studio is: the page, what is selected on it, and (from search) a setting to reveal.
// `go` re-mounts the page, so a page reads its starting selection from the route once.

import { createContext, use, useCallback, useEffect, useMemo, useState, type ReactNode } from "react";

export type PageId = "desktop" | "themes" | "widgets" | "bars" | "windows" | "settings";

/** What can be selected on the desktop stage. */
export type Target = { kind: "wallpaper" } | { kind: "widget"; instance: string } | { kind: "bar"; bar: "top" | "dock" };

export type WindowsTarget = "style" | "menu";

export type Route = (
  | { page: "desktop"; select?: Target }
  | { page: "themes"; view?: "gallery" | "new" }
  | { page: "themes"; view: "edit"; theme: string }
  | { page: "widgets"; select?: string; adding?: boolean }
  | { page: "bars" }
  | { page: "windows"; select?: WindowsTarget }
  | { page: "settings" }
) & {
  /** A setting id (`data-setting`) to scroll to and flash, from search. */
  focus?: string;
};

type Nav = {
  route: Route;
  /** Bumped by every `go`: the shell keys the page with it. */
  visit: number;
  go: (route: Route) => void;
};

const NavContext = createContext<Nav | null>(null);

export function useNav(): Nav {
  const nav = use(NavContext);
  if (!nav) throw new Error("useNav must be used inside <NavProvider>");
  return nav;
}

const REVEAL_TRIES = 30;

/**
 * After a `go` with `focus`: waits for that setting to render (pages load their data), opens the
 * "More options" around it, scrolls it into view and flashes it.
 */
export function useRevealFocus() {
  const { route, visit } = useNav();
  const focus = route.focus;
  useEffect(() => {
    if (!focus) return;
    let tries = 0;
    const timer = window.setInterval(() => {
      const element = document.querySelector<HTMLElement>(`[data-setting="${CSS.escape(focus)}"]`);
      if (!element && ++tries < REVEAL_TRIES) return;
      window.clearInterval(timer);
      if (!element) return;
      for (let parent = element.parentElement; parent; parent = parent.parentElement) if (parent instanceof HTMLDetailsElement) parent.open = true;
      element.scrollIntoView({ block: "center", behavior: "smooth" });
      element.classList.remove("flash");
      void element.offsetWidth;
      element.classList.add("flash");
      element.querySelector<HTMLElement>("input, button, select, [tabindex]")?.focus({ preventScroll: true });
    }, 60);
    return () => window.clearInterval(timer);
  }, [focus, visit]);
}

export function NavProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<{ route: Route; visit: number }>({ route: { page: "desktop" }, visit: 0 });
  const go = useCallback((route: Route) => setState((current) => ({ route, visit: current.visit + 1 })), []);
  const value = useMemo<Nav>(() => ({ ...state, go }), [state, go]);
  return <NavContext value={value}>{children}</NavContext>;
}
