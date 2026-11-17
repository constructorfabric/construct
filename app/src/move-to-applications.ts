export interface MoveOffer {
  /** Whether the app runs from a place that calls for the offer at all. */
  readonly isEligible: () => boolean;
  /** `true` once the user said "Not now" or a move did not happen. */
  readonly isDeclined: () => boolean;
  readonly markDeclined: () => void;
  /** Asks the user; `true` for "Move to Applications". */
  readonly ask: () => Promise<boolean>;
  /** Electron's move. It relaunches the moved copy when it returns `true`. */
  readonly move: (conflictHandler: () => boolean) => boolean;
}

/**
 * Offers to move the app to Applications. An installed copy is never replaced, and a "Not now"
 * or a move that did not happen is remembered, so the question is not asked at every launch.
 * Returns `true` when the app is moving and this run must stop.
 */
export async function offerMoveToApplications(offer: MoveOffer): Promise<boolean> {
  // Remembering is best-effort: a read-only or full data folder must not stop the app starting.
  const remember = () => {
    try {
      offer.markDeclined();
    } catch (error) {
      console.error(error);
    }
  };
  // The one guard: nothing in the offer may keep the app from starting.
  try {
    if (!offer.isEligible() || offer.isDeclined()) {
      return false;
    }
    if (!(await offer.ask())) {
      remember();
      return false;
    }
    try {
      if (offer.move(() => false)) {
        return true;
      }
    } catch (error) {
      console.error(error);
    }
    remember();
  } catch (error) {
    console.error(error);
  }
  return false;
}
