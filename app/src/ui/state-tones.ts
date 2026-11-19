export const tones = { running: 'success', stopped: 'danger', unknown: 'neutral' } as const;

/** Green for a tool that collects or reads its files, grey for one switched off, red for a broken one. */
export const connectorTones = {
  collecting: 'success',
  scanning: 'success',
  off: 'neutral',
  failed: 'danger',
} as const;
