export const tones = { running: 'success', stopped: 'danger', unknown: 'neutral' } as const;

/** Green for a tool that collects or reads its files, grey for one switched off, red for a broken one. */
export const connectorTones = {
  collecting: 'success',
  scanning: 'success',
  off: 'neutral',
  failed: 'danger',
} as const;

/** The apps and websites collection: a missing permission is as red as a failure. */
export const screenTones = {
  collecting: 'success',
  off: 'neutral',
  needs_permission: 'danger',
  failed: 'danger',
} as const;
