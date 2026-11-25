import WebKit

/// The permissions a page can ask for that the app has no way to refuse in code of its own, and the script that
/// refuses them.
///
/// The camera, the microphone and the motion sensors are refused in `GlasshouseModel`'s delegate. A page's request for
/// the location has no delegate call: the system asks the person only when the app has a text for it, and the app has
/// none, so the request would never be answered. This script answers it at once, as the person who says no would:
/// "permission denied", the same word the page gets from a phone that refuses.
enum GlasshouseRefusals {
    static let script = """
    (function () {
      var denied = {code: 1, message: 'User denied Geolocation', PERMISSION_DENIED: 1,
                    POSITION_UNAVAILABLE: 2, TIMEOUT: 3};
      var watches = 0;
      var refuse = function (success, failure) {
        if (typeof failure === 'function') { setTimeout(function () { failure(denied); }, 0); }
      };
      var refused = {
        getCurrentPosition: refuse,
        watchPosition: function (success, failure) { refuse(success, failure); return ++watches; },
        clearWatch: function () {}
      };
      try {
        Object.defineProperty(navigator, 'geolocation', {value: refused, configurable: true});
      } catch (error) {}
    })();
    """

    /// Runs in every page and every frame of it, before the page's own scripts.
    static func userScript() -> WKUserScript {
        WKUserScript(source: script, injectionTime: .atDocumentStart, forMainFrameOnly: false)
    }
}
