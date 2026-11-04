// The network flyout (docs/bars.md, "Flyouts" → "Network"): Wi-Fi on/off and the networks in
// range (signal, security; connect with a saved profile or a password), Ethernet's state, and VPN.
// Windows 11 24H2+ shows desktop apps Wi-Fi networks only with location access on: then the list
// says so and offers the privacy page.

import { button, glyph, h } from "../dom";
import { defineFlyout } from "../flyout";
import { fire, request } from "../host";
import type { NetworkInfo } from "../protocol";
import { glyphs, wifiBars } from "./glyphs";
import { bars } from "./model";
import type { QuickState, WifiNetwork, WifiState } from "./protocol";
import { show } from "./surface";
import { header, iconButton, notice, row, settingsLink } from "./ui";

export async function open(anchor: HTMLElement): Promise<void> {
  await show(network, null, { anchor, align: "end", extent: 520 });
}

const SECURITY: Record<WifiNetwork["security"], string> = {
  open: "Open",
  wpa2: "Secured",
  wpa3: "Secured (WPA3)",
  wpa: "Secured (WPA)",
  enterprise: "Secured (enterprise)",
  other: "Secured",
};

const network = defineFlyout<null>("network", (_props, ctx) => {
  const toggle = button("fly-switch");
  toggle.setAttribute("role", "switch");
  toggle.title = "Wi-Fi";
  const rescan = iconButton(glyphs.refresh, "Scan again", () => load(true));
  const list = h("div", "fly-list net-list");
  const wired = h("div", "net-wired");
  const vpn = row(h("span", "fly-icon fly-glyph-icon", glyph(glyphs.vpn)), "VPN", "Connections in Settings");
  vpn.addEventListener("click", () => fire("openSettings", { uri: "ms-settings:network-vpn" }));
  const root = h(
    "div",
    "fly-panel net",
    header("Wi-Fi", rescan, toggle),
    list,
    wired,
    vpn,
    h("div", "fly-footer", settingsLink("Network & internet settings", "ms-settings:network-status")),
  );

  let radioOn: boolean | undefined;
  let expanded: string | undefined;
  let state: WifiState = { networks: [] };

  const setRadio = (on: boolean | undefined) => {
    radioOn = on;
    toggle.hidden = on === undefined;
    toggle.setAttribute("aria-checked", String(on ?? false));
    toggle.dataset.on = String(on ?? false);
  };
  toggle.addEventListener("click", () => {
    if (radioOn === undefined) return;
    request("setRadio", { kind: "wifi", on: !radioOn })
      .then((quick: QuickState) => {
        setRadio(quick.radios.find((r) => r.kind === "wifi")?.on);
        load(true);
      })
      .catch((error: unknown) => console.warn("[bars] setRadio failed", error));
  });

  const connectForm = (net: WifiNetwork): HTMLElement => {
    const form = h("form", "net-connect");
    const needsPassword = !net.known && (net.security === "wpa2" || net.security === "wpa3" || net.security === "wpa");
    const settingsOnly = net.security === "enterprise" || net.security === "other";
    const status = h("span", "net-status");
    if (net.connected) {
      const disconnect = button("fly-button", "Disconnect");
      disconnect.addEventListener("click", () => {
        status.textContent = "Disconnecting…";
        request("wifiDisconnect")
          .then(() => load(false))
          .catch((error: unknown) => (status.textContent = String(error)));
      });
      form.append(disconnect, status);
      return form;
    }
    if (settingsOnly) {
      form.append(settingsLink("Connect in Settings", "ms-settings:network-wifi"));
      return form;
    }
    const password = h("input", "fly-input");
    password.type = "password";
    password.placeholder = "Network security key";
    password.autocomplete = "off";
    password.hidden = !needsPassword;
    const connect = button("fly-button primary", "Connect");
    connect.type = "submit";
    form.append(password, h("div", "net-connect-row", connect, status));
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      status.textContent = "Connecting…";
      connect.disabled = true;
      request("wifiConnect", { ssid: net.ssid, ...(needsPassword ? { password: password.value } : {}) })
        .then(() => {
          status.textContent = "Connecting…";
          setTimeout(() => load(false), 1500);
        })
        .catch((error: unknown) => {
          status.textContent = String(error).replace(/^Error: \w+: /, "");
          connect.disabled = false;
        });
    });
    if (needsPassword) requestAnimationFrame(() => password.focus());
    return form;
  };

  const render = () => {
    if (state.error === "location") {
      list.replaceChildren(
        notice("Windows shows apps Wi-Fi networks only with location access on.", settingsLink("Location settings", "ms-settings:privacy-location")),
      );
    } else if (state.error === "noAdapter") {
      list.replaceChildren(notice("No Wi-Fi adapter"));
      setRadio(undefined);
      rescan.hidden = true;
    } else if (state.error) {
      list.replaceChildren(notice("Wi-Fi isn't available right now"));
    } else if (state.networks.length === 0) {
      list.replaceChildren(notice(radioOn === false ? "Wi-Fi is off" : "Looking for networks…"));
    } else {
      list.replaceChildren(
        ...state.networks.map((net) => {
          const icon = h("span", "fly-icon fly-glyph-icon net-signal", glyph(wifiBars(bars(net.signal))));
          const lock = net.security === "open" ? [] : [glyph(glyphs.lock, "net-lock")];
          const detail = net.connected ? "Connected" : SECURITY[net.security];
          const item = row(icon, net.ssid, detail, ...lock);
          item.classList.toggle("is-current", net.connected);
          const wrap = h("div", "net-item", item);
          if (expanded === net.ssid) wrap.append(connectForm(net));
          item.addEventListener("click", () => {
            expanded = expanded === net.ssid ? undefined : net.ssid;
            render();
          });
          return wrap;
        }),
      );
    }
    ctx.reposition();
  };

  const load = (scan: boolean) => {
    request("wifi", { scan })
      .then((next) => {
        state = next;
        render();
      })
      .catch((error: unknown) => console.warn("[bars] wifi failed", error));
  };

  const renderWired = (info: NetworkInfo) => {
    wired.replaceChildren(
      ...(info.kind === "ethernet" ? [row(h("span", "fly-icon fly-glyph-icon", glyph(glyphs.ethernet)), "Ethernet", "Connected")] : []),
    );
  };

  request("quick")
    .then((quick) => {
      setRadio(quick.radios.find((r) => r.kind === "wifi")?.on);
      render();
    })
    .catch(() => setRadio(undefined));
  load(true);
  ctx.scope.on("wifi", (next) => {
    state = next;
    render();
  });
  ctx.scope.on("network", renderWired);
  for (const topic of ["wifi", "network"] as const) {
    fire("watch", { topic });
    ctx.scope.add(() => fire("unwatch", { topic }));
  }
  render();
  return root;
});
