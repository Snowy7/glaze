# OBS Studio component

`obs--controls` replaces the built-in OBS widget. Its three sizes use the shell's kit renderer.
The medium and large faces can select any scene; record, stream, pause/resume, and microphone
buttons send obs-websocket 5 requests only when activated. CPU, bitrate and dropped frames appear
when OBS supplies them. Bitrate needs two stream-byte samples, five seconds apart.

Enable **Tools → WebSocket Server Settings** in OBS. The plugin connects to
`ws://127.0.0.1:4455` with the `obswebsocket.json` subprotocol. Set `port` in the plugin's settings
for another local port. The manifest permission resolves that numeric setting when the component
loads; it never grants another host. Save a requested WebSocket password through the face's
password field; the runtime's secrets broker keeps it in Windows Credential Manager.

Closing OBS shows **OBS isn't running** and removes the controls. Retry uses the header refresh
button; automatic retries are thirty seconds apart. Closing the last face or suspending the
desktop closes the socket and cancels every timer. No native OBS helper is launched.

Build with `scripts/build-plugins.ps1` (own workspace, `wasm32-wasip2`). Protocol and view tests:
`cargo test --manifest-path plugins/obs/Cargo.toml`. The host's `obs_` tests load the real component,
check scoped sockets, offline behavior and memory. The shell's `obs_plugin_faces` test renders
`golden/host-sizes.png`; `MYPAPER_BLESS=1` updates the fixture.
