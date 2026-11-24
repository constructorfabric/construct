/**
 * The Chromium features the app turns off, as the value of the `disable-features` switch, or `null`
 * when there are none on this platform.
 *
 * On macOS, Electron 45 (Chromium 156) draws a text atlas wrongly with Skia's Graphite renderer: the
 * WebGL renderer of xterm.js, which fills a texture from glyphs drawn on a canvas, shows tiny glyphs
 * in Glasshouse pages (code-server, xtermjs.org). Google Chrome 156 draws the same pages correctly.
 * Without Graphite the page draws correctly and the GPU still does the work.
 */
export function disabledChromiumFeatures(platform: NodeJS.Platform): string | null {
  return platform === 'darwin' ? 'SkiaGraphite' : null;
}
