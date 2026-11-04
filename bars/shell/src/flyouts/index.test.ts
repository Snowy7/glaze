import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  fire: vi.fn(),
  open: vi.fn<(anchor: unknown) => Promise<void>>(),
  on: vi.fn(),
  closeFlyout: vi.fn(),
  flyoutOpen: vi.fn(() => false),
}));

vi.mock("../host", () => ({ fire: mocks.fire, on: (...args: unknown[]) => mocks.on(...args) }));
vi.mock("../flyout", () => ({ closeFlyout: mocks.closeFlyout, flyoutOpen: mocks.flyoutOpen }));
vi.mock("./launcher/launcher", () => ({ open: mocks.open }));
vi.mock("./notifications", () => ({ open: () => Promise.reject(new Error("provider failed")) }));
vi.mock("./modules", () => ({ bellModule: vi.fn(), languageModule: vi.fn(), trayModule: vi.fn() }));
vi.mock("./surface", () => ({ initSurfaces: vi.fn(), setSurfaceConfig: vi.fn() }));

import { openLauncher, openNotifications, toggleLauncher } from ".";

/** Lets the opener's promise chain settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.open.mockResolvedValue(undefined);
  mocks.flyoutOpen.mockReturnValue(false);
  vi.stubGlobal("document", { querySelector: () => null, body: "body" });
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

describe("a flyout that can't open", () => {
  it("tells the host, which logs it and opens the native panel", async () => {
    openNotifications({} as HTMLElement);
    await settle();
    expect(mocks.fire).toHaveBeenCalledWith("flyoutFailed", {
      flyout: "notifications",
      error: "Error: provider failed",
    });
  });

  it("does nothing when it opens", async () => {
    openLauncher({} as HTMLElement);
    await settle();
    expect(mocks.open).toHaveBeenCalled();
    expect(mocks.fire).not.toHaveBeenCalled();
  });
});

describe("toggleLauncher (the Windows key and the `start` action)", () => {
  it("opens from the start module when the bar has one", async () => {
    const start = {};
    vi.stubGlobal("document", { querySelector: (sel: string) => (sel.includes("start") ? start : null), body: "body" });
    toggleLauncher({} as HTMLElement);
    await settle();
    expect(mocks.open).toHaveBeenCalledWith(start);
  });

  it("opens from the module an action is bound on when there is no start module", async () => {
    const clock = {} as HTMLElement;
    toggleLauncher(clock);
    await settle();
    expect(mocks.open).toHaveBeenCalledWith(clock);
  });

  it("closes the launcher when it's open", async () => {
    mocks.flyoutOpen.mockReturnValue(true);
    toggleLauncher();
    await settle();
    expect(mocks.closeFlyout).toHaveBeenCalled();
    expect(mocks.open).not.toHaveBeenCalled();
  });
});
