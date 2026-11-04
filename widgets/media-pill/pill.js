// @ts-check
// The now-playing pill's pure logic: what to show for a track, when the title needs to scroll and
// how fast, and how far along the track is (extrapolated between the host's updates).

/**
 * @typedef {Object} Track
 * @property {string} title
 * @property {string} artist
 * @property {string} [album]
 * @property {string} app the playing app, for example "Spotify.exe"
 */

/**
 * The app's name from its executable: "Spotify.exe" gives "Spotify".
 * @param {string} app
 */
export function appName(app) {
  return app.trim().replace(/\.exe$/i, "");
}

/**
 * The two lines to show: the title (the app's name when the track has none) and the artist (the
 * album when there is no artist, else nothing).
 * @param {Track} track
 * @returns {{ title: string, artist: string }}
 */
export function describe(track) {
  return {
    title: track.title.trim() || appName(track.app) || "Unknown",
    artist: track.artist.trim() || (track.album ?? "").trim(),
  };
}

/**
 * Whether text wider than its box has to scroll (a pixel of slack ignores rounding).
 * @param {number} contentWidth
 * @param {number} boxWidth
 */
export function needsMarquee(contentWidth, boxWidth) {
  return boxWidth > 0 && contentWidth - boxWidth > 1;
}

/**
 * The duration of one way of a back-and-forth scroll over `distance` px: the text moves at a calm
 * speed for most of it (the animation holds at both ends), within sensible bounds.
 * @param {number} distance
 * @param {number} [pxPerSecond]
 */
export function marqueeSeconds(distance, pxPerSecond = 30) {
  const moving = distance / pxPerSecond;
  // The keyframes spend 72% of a leg moving and the rest holding still.
  return Math.min(60, Math.max(4, Math.round((moving / 0.72) * 10) / 10));
}

/**
 * @typedef {Object} Sample
 * @property {number | undefined} position seconds, as of `at`
 * @property {number | undefined} duration seconds
 * @property {boolean} playing
 * @property {number} at when the position was read, ms
 */

/**
 * How far along the track is, 0 to 1, moving on from the last sample while it plays; `null` when the
 * source reports no usable position or length (a live stream).
 * @param {Sample} sample
 * @param {number} now ms
 * @returns {number | null}
 */
export function progressAt({ position, duration, playing, at }, now) {
  if (duration === undefined || position === undefined) return null;
  if (!Number.isFinite(duration) || !Number.isFinite(position) || duration <= 0 || position < 0) return null;
  const elapsed = playing ? Math.max(0, now - at) / 1000 : 0;
  return Math.min(1, (position + elapsed) / duration);
}

/**
 * Seconds as a clock: `3:07`, or `1:02:03` past an hour.
 * @param {number} seconds
 */
export function formatClock(seconds) {
  const total = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}
