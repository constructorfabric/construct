/** What `app.commandLine` offers that the switches here need. */
export interface CommandLine {
  appendSwitch: (name: string, value?: string) => void;
}

/**
 * Turns off the Chromium features that break the app's pages. Call it before the app is ready.
 * On macOS, Skia Graphite breaks the glyph atlas of the xterm.js WebGL renderer.
 */
export function applyChromiumFeatures(commandLine: CommandLine, platform: NodeJS.Platform): void {
  if (platform === 'darwin') {
    commandLine.appendSwitch('disable-features', 'SkiaGraphite');
  }
}
