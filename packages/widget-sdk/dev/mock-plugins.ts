// Built-in mock plugins for the harness, so widgets can be developed without the real binary.
// The response shapes match the real plugin's (see docs/writing-plugins.md).

import { MypaperError } from "../src";

type Params = Record<string, unknown>;

const ICONS = ["clear-day", "partly-cloudy-day", "cloudy", "rain", "clear-day", "snow", "thunderstorm"] as const;
const CONDITIONS = ["Clear sky", "Partly cloudy", "Overcast", "Light rain", "Mainly clear", "Light snow", "Thunderstorm"];
const CODES = [0, 2, 3, 61, 1, 71, 95];

function asParams(value: unknown): Params {
  return typeof value === "object" && value !== null ? { ...value } : {};
}

function weather(method: string, raw: unknown): unknown {
  const p = asParams(raw);
  const imperial = p["units"] === "imperial";
  const t = (c: number) => Math.round((imperial ? c * 1.8 + 32 : c) * 10) / 10;
  const city = typeof p["city"] === "string" && p["city"] ? p["city"] : null;
  if (city?.toLowerCase() === "nowhere") throw new MypaperError("NotFound", `no place called "${city}"`);
  const location = {
    name: city ?? "Mockville",
    country: "Testland",
    lat: typeof p["lat"] === "number" ? p["lat"] : 51.5,
    lon: typeof p["lon"] === "number" ? p["lon"] : -0.12,
  };
  const units = imperial
    ? { temperature: "°F", speed: "mph", precipitation: "in" }
    : { temperature: "°C", speed: "km/h", precipitation: "mm" };

  if (method === "current") {
    return {
      location,
      units,
      time: new Date().toISOString().slice(0, 16),
      temperature: t(11.4),
      apparent: t(9.1),
      humidity: 78,
      wind: imperial ? 9 : 14.4,
      wind_direction: 230,
      precipitation: 0,
      code: 2,
      condition: "Partly cloudy",
      icon: "partly-cloudy-day",
      is_day: true,
    };
  }
  if (method === "forecast") {
    const count = typeof p["days"] === "number" ? Math.min(7, Math.max(1, p["days"])) : 5;
    const days = Array.from({ length: count }, (_, i) => {
      const date = new Date(Date.now() + i * 86_400_000).toISOString().slice(0, 10);
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
    return { location, units, days };
  }
  throw new MypaperError("NotFound", `weather has no method "${method}"`);
}

const mocks: Record<string, (method: string, params: unknown) => unknown> = { weather };

/** A widget folder can ship `mypaper.mock.json`: `{ "<plugin>": { "<method>": <result> } }`. */
export type FolderMocks = Record<string, Record<string, unknown>>;

export async function mockPlugin(id: string, method: string, params: unknown, folder: FolderMocks = {}): Promise<unknown> {
  await new Promise((r) => setTimeout(r, 120)); // feel like a process
  const fromFolder = folder[id]?.[method];
  if (fromFolder !== undefined) return fromFolder;
  const builtin = mocks[id];
  if (!builtin) {
    throw new MypaperError("PluginUnavailable", `no mock for plugin "${id}": add one to mypaper.mock.json or switch the harness to the real plugin`);
  }
  return builtin(method, params);
}
