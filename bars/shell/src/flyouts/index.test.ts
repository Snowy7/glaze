import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  fire: vi.fn(),
  open: vi.fn<(anchor: unknown) => Promise<void>>(),
  notificationsOpen: vi.fn<(anchor: unknown, keyboard?: boolean) => Promise<void>>(),
  quickOpen: vi.fn<(anchor: unknown, keyboard?: boolean) => Promise<void>>(),
  /** The handlers the page registered with `on`, by event name. */
  handlers: new Map<string, (data: { flyout: string }) => void>(),
  closeFlyout: vi.fn(),
  flyoutOpen: vi.fn((_id?: string) => false),
}));

vi.mock("../host", () => ({
  fire: mocks.fire,
  on: (name: string, handler: (data: { flyout: string }) => void) => mocks.handlers.set(name, handler),
}));
vi.mock("../flyout", () => ({ closeFlyout: mocks.closeFlyout, flyoutOpen: mocks.flyoutOpen }));
vi.mock("./launcher/launcher", () => ({ open: mocks.open }));
vi.mock("./notifications", () => ({ open: mocks.notificationsOpen }));
vi.mock("./quick", () => ({ open: mocks.quickOpen }));
vi.mock("./modules", () => ({ bellModule: vi.fn(), languageModule: vi.fn(), trayModule: vi.fn() }));
vi.mock("./surface", () => ({ initSurfaces: vi.fn(), setSurfaceConfig: vi.fn() }));

import { openLauncher, openNotifications, toggleLauncher } from ".";

/** Lets the opener's promise chain settle. */
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  vi.clearAllMocks();
  mocks.open.mockResolvedValue(undefined);
  mocks.quickOpen.mockResolvedValue(undefined);
  mocks.notificationsOpen.mockResolvedValue(undefined);
  mocks.flyoutOpen.mockReturnValue(false);
  vi.stubGlobal("document", { querySelector: () => null, querySelectorAll: () => [], body: "body" });
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

describe("Win+N and Win+A (the host's `open` event)", () => {
  const press = (flyout: string) => mocks.handlers.get("open")?.({ flyout });
  const bar = (modules: Record<string, unknown>, all: unknown[] = []) =>
    vi.stubGlobal("document", {
      querySelector: (selector: string) =>
        Object.entries(modules).find(([name]) => selector.includes(`"${name}"`))?.[1] ?? null,
      querySelectorAll: () => all,
      body: "body",
    });

  it("opens from the flyout's own module, taking the keyboard", async () => {
    const bell = {};
    bar({ notifications: bell });
    press("notifications");
    await settle();
    expect(mocks.notificationsOpen).toHaveBeenCalledWith(bell, true);
  });

  it("opens from the bar's last module when it has none of its own", async () => {
    const [first, last] = [{}, {}];
    bar({}, [first, last]);
    press("quick");
    await settle();
    expect(mocks.quickOpen).toHaveBeenCalledWith(last, true);
  });

  it("closes the flyout when it's already open", async () => {
    mocks.flyoutOpen.mockImplementation((id) => id === "quick");
    press("quick");
    await settle();
    expect(mocks.closeFlyout).toHaveBeenCalled();
    expect(mocks.quickOpen).not.toHaveBeenCalled();
  });
});

describe("a flyout that can't open", () => {
  it("tells the host, which logs it and opens the native panel", async () => {
    mocks.notificationsOpen.mockRejectedValue(new Error("provider failed"));
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
