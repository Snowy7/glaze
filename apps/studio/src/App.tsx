import { X } from "lucide-react";
import { useEffect, useState } from "react";

import { Sidebar, type View } from "./components/Sidebar";
import { api } from "./lib/api";
import { StudioProvider, useStudio } from "./lib/studio";
import { Gallery } from "./views/Gallery";
import { NewTheme } from "./views/NewTheme";
import { Settings } from "./views/Settings";

export function App() {
  useMicaBackdrop();
  return (
    <StudioProvider>
      <Shell />
    </StudioProvider>
  );
}

/** Lets the Mica backdrop show through, but only once the backend confirms it's active. */
function useMicaBackdrop() {
  useEffect(() => {
    api
      .shellInfo()
      .then(({ mica }) => {
        document.documentElement.dataset.mica = String(mica);
      })
      .catch(() => {});
  }, []);
}

function Shell() {
  const [view, setView] = useState<View>("gallery");
  return (
    <div className="flex h-full">
      <Sidebar view={view} onNavigate={setView} />
      <main
        // Re-key per view so each one fades in.
        key={view}
        className="min-w-0 flex-1 animate-[rise_260ms_ease-out_backwards] overflow-y-auto rounded-tl-2xl border-t border-l border-line bg-canvas px-9 pt-9 pb-8"
      >
        {view === "gallery" && <Gallery onNavigate={setView} />}
        {view === "new" && <NewTheme onNavigate={setView} />}
        {view === "settings" && <Settings />}
      </main>
      <Toaster />
    </div>
  );
}

function Toaster() {
  const { toast, dismissToast } = useStudio();
  return (
    <div aria-live="polite" className="pointer-events-none fixed inset-x-0 bottom-6 flex justify-center px-6">
      {toast && (
        <div
          key={toast.id}
          role={toast.tone === "error" ? "alert" : "status"}
          className={`selectable pointer-events-auto flex max-w-xl animate-[rise_220ms_ease-out_backwards] items-start gap-3 rounded-xl bg-surface-hi py-2.5 pr-2 pl-4 text-[13px] shadow-card ring-1 ${
            toast.tone === "error" ? "text-danger ring-danger/40" : "text-ink ring-line-strong"
          }`}
        >
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
