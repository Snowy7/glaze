// dist/harden.js: the host injects it into every frame at document creation, before the page's own
// scripts. WebRTC traffic bypasses the host's network filter, so it is removed everywhere (including
// frames nested inside a widget). Self-contained classic script: no imports, no API.

(() => {
  for (const name of ["RTCPeerConnection", "webkitRTCPeerConnection", "RTCDataChannel", "RTCSessionDescription", "RTCIceCandidate"]) {
    try {
      Object.defineProperty(window, name, { value: undefined, writable: false, configurable: false });
    } catch {
      // Already locked down: nothing to do.
    }
  }
})();
