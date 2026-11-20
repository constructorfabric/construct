import { copyableText, systemPaneAddress, webAddress } from './external';

/**
 * A handler that opens what `gate` lets through with `open` and refuses anything else, whoever
 * asks. A target that cannot be opened is a refusal too, not an error the window has to catch.
 */
function createOpener(
  gate: (raw: unknown) => string | null,
  open: (address: string) => Promise<void>,
): (raw: unknown) => Promise<boolean> {
  return async (raw) => {
    const address = gate(raw);
    if (address === null) {
      return false;
    }
    try {
      await open(address);
    } catch (error) {
      console.error(error);
      return false;
    }
    return true;
  };
}

/** The handler of `openLink`: only a web address is opened. */
export function createLinkHandler(
  open: (address: string) => Promise<void>,
): (url: unknown) => Promise<boolean> {
  return createOpener(webAddress, open);
}

/** The handler of `openPane`: only a pane of System Settings from the fixed list is opened. */
export function createPaneHandler(
  open: (address: string) => Promise<void>,
): (pane: unknown) => Promise<boolean> {
  return createOpener(systemPaneAddress, open);
}

/**
 * The handler of `copyText`: it writes a text of a fitting size with `write` and refuses anything
 * else, whoever asks. A clipboard that cannot be written is a refusal too.
 */
export function createCopyHandler(write: (text: string) => void): (text: unknown) => boolean {
  return (text) => {
    const copyable = copyableText(text);
    if (copyable === null) {
      return false;
    }
    try {
      write(copyable);
    } catch (error) {
      console.error(error);
      return false;
    }
    return true;
  };
}
