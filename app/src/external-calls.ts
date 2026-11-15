import { webAddress } from './external';

/**
 * The handler of `openLink`: it opens a web address with `open` and refuses anything else, whoever
 * asks. A browser that cannot be started is a refusal too, not an error the window has to catch.
 */
export function createLinkHandler(
  open: (address: string) => Promise<void>,
): (url: unknown) => Promise<boolean> {
  return async (url) => {
    const address = webAddress(url);
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
