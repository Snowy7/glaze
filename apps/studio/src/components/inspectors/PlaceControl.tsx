import { LoaderCircle, MapPin, Search } from "lucide-react";
import { useEffect, useState } from "react";

import { api, errorText, type Place, type PlaceCandidate, type WidgetSetting } from "../../lib/api";
import type { Json } from "../../lib/barsSchema";
import { Button, Segmented, TextInput } from "../ui";

type PlaceSetting = Extract<WidgetSetting, { type: "place" }>;

const SEARCH_DELAY_MS = 300;
const MIN_QUERY = 2;

/** A saved value as a place: `null` is automatic; a plain string from an older text setting is `{ name }`. */
function placeOf(value: Json | undefined): Place | null {
  if (typeof value === "string") return value.trim() === "" ? null : { name: value };
  if (typeof value !== "object" || value === null || Array.isArray(value) || typeof value.name !== "string") return null;
  const number = (v: Json | undefined) => (typeof v === "number" ? v : undefined);
  const text = (v: Json | undefined) => (typeof v === "string" ? v : undefined);
  return { name: value.name, lat: number(value.lat), lon: number(value.lon), region: text(value.region), country: text(value.country) };
}

const isCandidate = (item: unknown): item is PlaceCandidate =>
  typeof item === "object" && item !== null && "name" in item && typeof item.name === "string" && "lat" in item && typeof item.lat === "number" && "lon" in item && typeof item.lon === "number";

/** What's saved for a picked town (docs/widgets.md, "Places"): never the typed text. */
function picked(candidate: PlaceCandidate): Json {
  return {
    name: candidate.name,
    lat: candidate.lat,
    lon: candidate.lon,
    ...(candidate.admin1 && { region: candidate.admin1 }),
    ...(candidate.country && { country: candidate.country }),
  };
}

const describe = (place: { region?: string | null; admin1?: string | null; country?: string | null }) => [place.region ?? place.admin1, place.country].filter(Boolean).join(", ");

type Search = { kind: "idle" } | { kind: "searching" } | { kind: "found"; results: PlaceCandidate[] } | { kind: "failed"; message: string };

/**
 * A `place` setting: Automatic (where this PC is), or a town. Typing searches with the widget's own
 * plugin method (`search`), about 300 ms after two characters; picking a result saves it.
 */
export function PlaceControl({ setting, value, widget, onChange }: { setting: PlaceSetting; value: Json | undefined; widget: string; onChange: (value: Json) => void }) {
  const place = placeOf(value);
  const [choosing, setChoosing] = useState(place !== null);
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState<Search>({ kind: "idle" });
  const needle = query.trim();

  useEffect(() => {
    const method = setting.search;
    if (!method || needle.length < MIN_QUERY) return setSearch({ kind: "idle" });
    let current = true;
    const timer = window.setTimeout(() => {
      setSearch({ kind: "searching" });
      api.pluginCall(widget, method.plugin, method.method, { query: needle }).then(
        (answer) => current && setSearch({ kind: "found", results: Array.isArray(answer) ? answer.filter(isCandidate).slice(0, 6) : [] }),
        (e: unknown) => current && setSearch({ kind: "failed", message: errorText(e) }),
      );
    }, SEARCH_DELAY_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [needle, setting.search, widget]);

  function choose(candidate: PlaceCandidate) {
    onChange(picked(candidate));
    setQuery("");
  }

  return (
    <div className="flex flex-col gap-2">
      <Segmented<"auto" | "town">
        label={setting.label}
        hideLabel
        size="sm"
        className="w-full"
        value={choosing ? "town" : "auto"}
        options={[
          { value: "auto", label: "Automatic" },
          { value: "town", label: "A town" },
        ]}
        onChange={(mode) => {
          setChoosing(mode === "town");
          if (mode === "auto" && place !== null) onChange(null);
        }}
      />
      {!choosing && <p className="text-[12px] text-ink-3">Where this PC is.</p>}
      {choosing && place && (
        <div className="flex items-center gap-2.5 rounded-lg bg-canvas px-2.5 py-2 ring-1 ring-line-strong ring-inset">
          <MapPin className="size-4 shrink-0 text-ink-2" aria-hidden />
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[13px] font-medium text-ink">{place.name}</span>
            {describe(place) && <span className="block truncate text-[12px] text-ink-3">{describe(place)}</span>}
          </span>
        </div>
      )}
      {choosing && setting.search && (
        <>
          <div className="relative">
            <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-ink-3" aria-hidden />
            <TextInput compact type="search" aria-label={`Find a town for ${setting.label}`} placeholder={place ? "Find another town" : "Find a town"} value={query} onChange={(e) => setQuery(e.currentTarget.value)} className="w-full pl-8" />
            {search.kind === "searching" && <LoaderCircle className="absolute top-1/2 right-2.5 size-4 -translate-y-1/2 animate-spin text-ink-3" aria-hidden />}
          </div>
          {search.kind === "found" &&
            (search.results.length === 0 ? (
              <p className="text-[12px] text-ink-3">No town called “{needle}”.</p>
            ) : (
              <ul aria-label="Towns" className="flex flex-col rounded-lg bg-canvas p-1 ring-1 ring-line-strong ring-inset">
                {search.results.map((candidate) => (
                  <li key={`${candidate.lat},${candidate.lon}`}>
                    <Button variant="ghost" size="sm" className="h-auto w-full justify-start py-1.5 text-left" onClick={() => choose(candidate)}>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-medium text-ink">{candidate.name}</span>
                        {describe(candidate) && <span className="block truncate text-[12px] font-normal text-ink-3">{describe(candidate)}</span>}
                      </span>
                    </Button>
                  </li>
                ))}
              </ul>
            ))}
          {search.kind === "failed" && <p className="text-[12px] text-ink-3">Search isn't available right now: {search.message}</p>}
        </>
      )}
    </div>
  );
}
