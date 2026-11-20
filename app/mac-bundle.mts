// What macOS needs from the packaged app before it lets the agent read the front window and the browser tabs.

/** The identity of the agent in the system's privacy lists, the same as its LaunchAgent label. */
export const agentIdentifier = 'cf.construct.agent';

/**
 * Added to the app's Info.plist. Without the description, macOS refuses the Apple Events that read browser tabs
 * and shows no prompt.
 */
export const macInfoPlist = {
  NSAppleEventsUsageDescription:
    'Construct Browser reads the title and the address of your browser tab to count the time you spend on each website. They stay on this computer.',
};

/**
 * The entitlements for signing under the Hardened Runtime, applied by the signing commands of the README (the build
 * itself signs nothing, so it holds no certificate): only the one that lets the agent send Apple Events. */
export const entitlementsFile = 'entitlements.plist';
