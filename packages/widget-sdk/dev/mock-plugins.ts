// Built-in mock plugins for the harness, so widgets can be developed without the real binary.
// The response shapes match the real plugin's (see docs/writing-plugins.md).

import { MypaperError } from "../src";

type Params = Record<string, unknown>;

const ICONS = ["clear-day", "partly-cloudy-day", "cloudy", "rain", "clear-day", "snow", "thunderstorm"] as const;
const CONDITIONS = ["Clear sky", "Partly cloudy", "Overcast", "Light rain", "Mainly clear", "Light snow", "Thunderstorm"];
const CODES = [0, 2, 3, 61, 1, 71, 95];

/** Weather "scenarios" the harness can show: set the weather widget's city to one of these keys. */
const SCENARIOS: Record<string, { code: number; condition: string; icon: string; night?: boolean; temp: number }> = {
  clear: { code: 0, condition: "Clear sky", icon: "clear-day", temp: 21.4 },
  night: { code: 0, condition: "Clear sky", icon: "clear-night", night: true, temp: 6.2 },
  partly: { code: 2, condition: "Partly cloudy", icon: "partly-cloudy-day", temp: 11.4 },
  cloudy: { code: 3, condition: "Overcast", icon: "cloudy", temp: 9.3 },
  fog: { code: 45, condition: "Fog", icon: "fog", temp: 4.8 },
  drizzle: { code: 53, condition: "Drizzle", icon: "drizzle", temp: 8.1 },
  rain: { code: 63, condition: "Rain", icon: "rain", temp: 7.6 },
  snow: { code: 73, condition: "Snow", icon: "snow", temp: -2.4 },
  storm: { code: 95, condition: "Thunderstorm", icon: "thunderstorm", temp: 16.9 },
};

/** Towns the mock `search` knows, and where `locate` says this PC is (?locate=ok|slow|fail). */
const TOWNS = [
  { name: "Kensington", admin1: "England", admin2: "Greater London", country: "United Kingdom", country_code: "GB", lat: 51.4991, lon: -0.1938, population: 64681 },
  { name: "Bergen", admin1: "Vestland", country: "Norway", country_code: "NO", lat: 60.3913, lon: 5.3221, population: 285601 },
  { name: "Bergen op Zoom", admin1: "North Brabant", country: "Netherlands", country_code: "NL", lat: 51.495, lon: 4.2917, population: 66354 },
  { name: "Springfield", admin1: "Illinois", country: "United States", country_code: "US", lat: 39.8017, lon: -89.6437, population: 114394 },
  { name: "Springfield", admin1: "Missouri", country: "United States", country_code: "US", lat: 37.2153, lon: -93.2982, population: 169176 },
  { name: "Tromsø", admin1: "Troms", country: "Norway", country_code: "NO", lat: 69.6496, lon: 18.957, population: 77544 },
];
let locating: "ok" | "slow" | "fail" = "ok";
/** The harness's `locate=` parameter: how the mock `locate` behaves. */
export function setMockLocate(mode: string | null): void {
  locating = mode === "fail" || mode === "slow" ? mode : "ok";
}

/** The mock town nearest to a point (for a location's name from coordinates). */
function nearest(lat: number, lon: number) {
  return TOWNS.reduce((best, t) => (Math.hypot(t.lat - lat, t.lon - lon) < Math.hypot(best.lat - lat, best.lon - lon) ? t : best));
}

function asParams(value: unknown): Params {
  return typeof value === "object" && value !== null ? { ...value } : {};
}

const pad = (n: number) => String(n).padStart(2, "0");
/** Local "YYYY-MM-DD" and "YYYY-MM-DDTHH:00", like open-meteo with timezone=auto. */
const localDate = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const localHour = (d: Date) => `${localDate(d)}T${pad(d.getHours())}:00`;

/** The weather names the hourly icons by day/night; mirror the plugin's mapping for the codes used here. */
function hourIcon(code: number, isDay: boolean): string {
  if (code === 0 || code === 1) return isDay ? "clear-day" : "clear-night";
  if (code === 2) return isDay ? "partly-cloudy-day" : "partly-cloudy-night";
  if (code === 3) return "cloudy";
  if (code === 45) return "fog";
  if (code >= 51 && code <= 57) return "drizzle";
  if (code >= 71 && code <= 77) return "snow";
  if (code === 95) return "thunderstorm";
  return "rain";
}

function weather(method: string, raw: unknown): unknown {
  const p = asParams(raw);
  if (method === "locate") {
    if (locating === "fail") throw new MypaperError("NotFound", "location unavailable");
    const here = TOWNS[0]!;
    return { ...here, source: "windows" };
  }
  if (method === "search") {
    const query = typeof p["query"] === "string" ? p["query"].trim().toLowerCase() : "";
    return query.length < 2 ? [] : TOWNS.filter((t) => t.name.toLowerCase().startsWith(query));
  }
  const imperial = p["units"] === "imperial";
  const t = (c: number) => Math.round((imperial ? c * 1.8 + 32 : c) * 10) / 10;
  const city = typeof p["city"] === "string" && p["city"] ? p["city"] : null;
  const key = city?.toLowerCase() ?? "";
  if (key === "nowhere") throw new MypaperError("NotFound", `no place called "${city}"`);
  if (key === "offline") throw new MypaperError("PluginUnavailable", "weather service unreachable");
  const scenario = SCENARIOS[key] ?? SCENARIOS["partly"]!;
  const lat = typeof p["lat"] === "number" ? p["lat"] : 51.5;
  const lon = typeof p["lon"] === "number" ? p["lon"] : -0.12;
  const location = {
    name: city ?? nearest(lat, lon).name,
    country: city ? "Testland" : nearest(lat, lon).country,
    lat,
    lon,
  };
  const units = imperial
    ? { temperature: "°F", speed: "mph", precipitation: "in" }
    : { temperature: "°C", speed: "km/h", precipitation: "mm" };
  const now = new Date();

  if (method === "current") {
    return {
      location,
      units,
      time: localHour(now),
      temperature: t(scenario.temp),
      apparent: t(scenario.temp - 2.3),
      humidity: 78,
      wind: imperial ? 9 : 14.4,
      wind_direction: 230,
      precipitation: scenario.code >= 51 ? 1.2 : 0,
      code: scenario.code,
      condition: scenario.condition,
      icon: scenario.icon,
      is_day: !scenario.night,
    };
  }
  if (method === "forecast") {
    const count = typeof p["days"] === "number" ? Math.min(7, Math.max(1, p["days"])) : 5;
    const days = Array.from({ length: count }, (_, i) => {
      const date = localDate(new Date(now.getTime() + i * 86_400_000));
      const k = (i * 3 + 1) % CODES.length;
      return {
        date,
        code: CODES[k],
        condition: CONDITIONS[k],
        icon: ICONS[k],
        min: t(4 + ((i * 5) % 6)),
        max: t(10 + ((i * 7) % 9)),
        precipitation_probability: [10, 30, 70, 90, 5, 40, 60][i % 7],
      };
    });
    // 24 hours from the current one: a gentle temperature wave, the scenario's weather for the first
    // hours, then a change, and day/night from the local hour.
    const hourly = Array.from({ length: 24 }, (_, i) => {
      const at = new Date(now.getFullYear(), now.getMonth(), now.getDate(), now.getHours() + i);
      const isDay = at.getHours() >= 7 && at.getHours() < 19;
      const code = i < 9 ? scenario.code : (CODES[(i + 2) % CODES.length] ?? 0);
      const wet = code >= 51;
      return {
        time: localHour(at),
        temperature: t(scenario.temp + 3.2 * Math.sin(((at.getHours() - 9) / 24) * Math.PI * 2)),
        code,
        condition: i < 9 ? scenario.condition : (CONDITIONS[(i + 2) % CODES.length] ?? ""),
        icon: i === 0 ? scenario.icon : hourIcon(code, isDay),
        ...(i % 5 === 4 ? {} : { precipitation_probability: wet ? 40 + ((i * 11) % 55) : (i * 7) % 25 }),
        is_day: i === 0 ? !scenario.night : isDay,
      };
    });
    return { location, units, days, hourly };
  }
  throw new MypaperError("NotFound", `weather has no method "${method}"`);
}

const mocks: Record<string, (method: string, params: unknown) => unknown> = { weather };

/** A widget folder can ship `mypaper.mock.json`: `{ "<plugin>": { "<method>": <result> } }`. */
export type FolderMocks = Record<string, Record<string, unknown>>;

export async function mockPlugin(id: string, method: string, params: unknown, folder: FolderMocks = {}): Promise<unknown> {
  // Feel like a process; the weather city "slow" never answers (to look at loading states).
  const slow = id === "weather" && (asParams(params)["city"] === "slow" || (method === "locate" && locating === "slow"));
  await new Promise((r) => setTimeout(r, slow ? 600_000 : 120));
  const fromFolder = folder[id]?.[method];
  if (fromFolder !== undefined) return fromFolder;
  const builtin = mocks[id];
  if (!builtin) {
    throw new MypaperError("PluginUnavailable", `no mock for plugin "${id}": add one to mypaper.mock.json or switch the harness to the real plugin`);
  }
  return builtin(method, params);
}
