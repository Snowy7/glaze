import { beforeEach, describe, expect, it, vi } from "vitest";

const calls = vi.hoisted(() => ({ flyout: vi.fn(), launcher: vi.fn(), fire: vi.fn() }));

vi.mock("./flyouts", () => ({
  openCalendar: (a: unknown) => calls.flyout("calendar", a),
  openMixer: (a: unknown) => calls.flyout("mixer", a),
  openNetwork: (a: unknown) => calls.flyout("network", a),
  openNotifications: (a: unknown) => calls.flyout("notifications", a),
  openQuick: (a: unknown) => calls.flyout("quick", a),
  toggleLauncher: (a: unknown) => calls.launcher(a),
}));
vi.mock("./host", () => ({ fire: calls.fire, request: vi.fn() }));

import { BUILTIN, runAction } from "./actions";
import { BUILTIN_ACTIONS, type BuiltinAction } from "./protocol";

const anchor = {} as HTMLElement;
const run = (action: BuiltinAction) => runAction("clock", "left", action, anchor);

beforeEach(() => vi.clearAllMocks());

describe("built-in action routing", () => {
  it("has a handler for every action but `default`", () => {
    expect(Object.keys(BUILTIN).sort()).toEqual(BUILTIN_ACTIONS.filter((a) => a !== "default").sort());
  });

  it.each([
    ["notifications", "notifications"],
    ["quick-settings", "quick"],
    ["calendar", "calendar"],
    ["network", "network"],
    ["mixer", "mixer"],
  ] as const)("`%s` opens our %s flyout from the module it's bound on", (action, flyout) => {
    run(action);
    expect(calls.flyout).toHaveBeenCalledWith(flyout, anchor);
    expect(calls.fire).not.toHaveBeenCalled();
  });

  it("`start` opens the launcher the Windows key opens", () => {
    run("start");
    expect(calls.launcher).toHaveBeenCalledWith(anchor);
    expect(calls.fire).not.toHaveBeenCalled();
  });

  it.each([
    ["native-start", "start", undefined],
    ["native-notifications", "actionCenter", { panel: "notifications" }],
    ["native-quick-settings", "actionCenter", { panel: "quick" }],
  ] as const)("`%s` opens Windows' own panel (%s)", (action, method, params) => {
    run(action);
    expect(calls.fire.mock.calls).toEqual([params ? [method, params] : [method]]);
    expect(calls.flyout).not.toHaveBeenCalled();
    expect(calls.launcher).not.toHaveBeenCalled();
  });

  it("sends exec/open actions to the host, which runs its own copy", () => {
    runAction("clock", "right", { open: "https://example.com" }, anchor);
    expect(calls.fire).toHaveBeenCalledWith("run", { module: "clock", trigger: "right" });
  });
});
