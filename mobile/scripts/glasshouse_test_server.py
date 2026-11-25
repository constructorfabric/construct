#!/usr/bin/env python3
"""The test server of the Glasshouse page tests: small made-up pages over https, on this computer.

It listens on 127.0.0.1 only, which the Android emulator reaches as 10.0.2.2. One run serves:

  trusted    https, with the certificate the debug build of the app trusts (`trusted.pem`)
  untrusted  https, with a certificate nothing trusts (`untrusted.pem`)
  plain      http, the same pages (the app blocks plain http; a phone's own browser opens it)
  flaky      https like `trusted`, but it only listens after `/__flaky?on=1` on `trusted` and stops after `on=0`

The first line it prints is one JSON object with the four base addresses. The certificates are the ones the Gradle
task `generateGlasshouseTestCertificates` writes. The pages:

  /                 a page with a script, a picture, a link and a text field
  /garden.js        the script of the page
  /garden.png       the picture of the page
  /herbs            a second page, the target of the links
  /signin           sets a session cookie and a cookie that lasts a day
  /account          says whether the cookies came back
  /new-window       a link and a button that ask for a new window
  /app-links        links to addresses of other apps and of phone features, and one that redirects to one
  /permissions      asks for the camera and the location and writes what it was told
  /slow             answers after thirty seconds
  /__requests       what the server was asked for: path, User-Agent and whether a cookie came (JSON)
  /__reset          forgets the requests
  /__counts         how many connections each site was asked for (JSON)
  /__flaky?on=1|0   starts or stops the `flaky` listener
"""

import argparse
import json
import socket
import ssl
import struct
import sys
import threading
import time
import zlib
from http.cookies import SimpleCookie
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

SLOW_SECONDS = 30
EMULATOR_HOST = "10.0.2.2"
APP_LINKS = {
    "intent": "intent://scan/#Intent;scheme=zxing;package=com.google.zxing.client.android;end",
    "market": "market://details?id=com.android.chrome",
    "tel": "tel:+15550100",
    "mailto": "mailto:someone@example.test",
    "custom": "gardenapp://open/herbs",
    "redirect": "/redirect-to-app",
}
PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>body{{font-family:Georgia,serif;margin:16px;color:#1f2933;line-height:1.5}}
img{{display:block;width:100%;height:150px;border-radius:8px}} a,button,input{{font-size:1rem}}
input{{width:90%;padding:8px}}</style></head><body>{body}</body></html>
"""


def page(title, body):
    return PAGE.format(title=title, body=body).encode()


def picture():
    """A plain green picture, as a PNG made here."""
    width, height = 16, 8
    rows = b"".join(b"\x00" + bytes((216, 227, 208)) * width for _ in range(height))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


class State:
    def __init__(self):
        self.lock = threading.Lock()
        self.requests = []
        self.connections = {}
        self.flaky = None
        self.make_flaky = None

    def log(self, site, path, agent, cookie):
        with self.lock:
            self.requests.append({"site": site, "path": path, "userAgent": agent, "cookie": cookie})

    def count(self, site):
        with self.lock:
            self.connections[site] = self.connections.get(site, 0) + 1

    def set_flaky(self, on):
        """Starts the `flaky` listener, or stops it so that connections to its port are refused."""
        with self.lock:
            if on and self.flaky is None and self.make_flaky is not None:
                self.flaky = self.make_flaky()
                threading.Thread(target=self.flaky.serve_forever, daemon=True).start()
                return
            stopping = None if on else self.flaky
            if not on:
                self.flaky = None
        # Outside the lock: the listener's own thread takes it for each connection while it winds down.
        if stopping is not None:
            stopping.shutdown()
            stopping.server_close()


def make_handler(site, state, context):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"
        server_version = "GlasshouseTest"

        def setup(self):
            if context is not None:
                # The handshake runs here, so a client that refuses the certificate costs this thread only.
                self.request.settimeout(10)
                self.request.do_handshake()
                self.request.settimeout(None)
            super().setup()

        def log_message(self, format, *args):  # noqa: A002
            pass

        def send(self, status, body, kind="text/html; charset=utf-8", headers=()):
            self.send_response(status)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            for name, value in headers:
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            url = urlsplit(self.path)
            path = url.path
            cookie = self.headers.get("Cookie", "")
            if path.startswith("/__"):
                return self.control(path, parse_qs(url.query))
            state.log(site, self.path, self.headers.get("User-Agent", ""), bool(cookie))
            if path == "/":
                body = (
                    "<h1>Garden notes</h1><p>Which herbs grow well on a sunny balcony, and how often to water them.</p>"
                    '<img id="pic" src="/garden.png" alt="">'
                    "<p>Basil likes warmth. Mint likes a pot of its own.</p>"
                    '<p><a id="herbs" href="/herbs">Read the herb list</a></p>'
                    '<p><input id="note" type="text" placeholder="A note"></p>'
                    '<script src="/garden.js"></script>'
                )
                return self.send(200, page("Garden notes", body))
            if path == "/garden.js":
                script = b"document.body.setAttribute('data-script', 'ran');"
                return self.send(200, script, "text/javascript")
            if path == "/garden.png":
                return self.send(200, picture(), "image/png")
            if path == "/herbs":
                body = '<h1>Herbs</h1><p>Basil, mint and parsley.</p><p><a id="back" href="/">Back to the notes</a></p>'
                return self.send(200, page("Herbs", body))
            if path == "/signin":
                headers = (
                    ("Set-Cookie", "session=garden-session; Path=/; HttpOnly"),
                    ("Set-Cookie", "remember=garden-remember; Path=/; Max-Age=86400"),
                )
                return self.send(200, page("Signed in", "<h1>Signed in</h1><p>Welcome back, Sam.</p>"), headers=headers)
            if path == "/account":
                jar = {name: morsel.value for name, morsel in SimpleCookie(cookie).items()}
                signed = jar.get("session") == "garden-session"
                remembered = jar.get("remember") == "garden-remember"
                text = "Signed in as Sam" if signed else "Signed out"
                body = f"<h1>{text}</h1><p>Session cookie: {'yes' if signed else 'no'}. Remember cookie: {'yes' if remembered else 'no'}.</p>"
                return self.send(200, page(text, body))
            if path == "/new-window":
                body = (
                    "<h1>New windows</h1>"
                    '<p><a id="blank" href="/herbs" target="_blank">Open the herbs in a new window</a></p>'
                    "<p><button id=\"open\" onclick=\"window.open('/herbs')\">Open the herbs from a script</button></p>"
                )
                return self.send(200, page("New windows", body))
            if path == "/app-links":
                links = "".join(f'<p><a id="{name}" href="{target}">Link {name}</a></p>' for name, target in APP_LINKS.items())
                # Counts the taps that reach the page, so a test can tell a tap that did nothing from one that never came.
                counter = (
                    "<script>document.addEventListener('click', function () {"
                    "document.body.setAttribute('data-clicks', String((Number(document.body.getAttribute('data-clicks')) || 0) + 1));"
                    "}, true);</script>"
                )
                return self.send(200, page("App links", "<h1>App links</h1>" + links + counter))
            if path == "/redirect-to-app":
                return self.send(302, b"", headers=(("Location", "market://details?id=com.android.chrome"),))
            if path == "/permissions":
                script = """
const lines = {};
const out = document.getElementById('result');
function show() {
  out.textContent = Object.keys(lines).sort().map((key) => key + ':' + lines[key]).join(' ');
  if (Object.keys(lines).length === 2) document.body.setAttribute('data-done', 'yes');
}
navigator.mediaDevices.getUserMedia({video: true}).then(() => { lines.camera = 'granted'; show(); },
  (error) => { lines.camera = error.name; show(); });
navigator.geolocation.getCurrentPosition(() => { lines.location = 'granted'; show(); },
  (error) => { lines.location = 'code' + error.code; show(); });
"""
                body = f'<h1>Permissions</h1><p id="result">asking</p><script>{script}</script>'
                return self.send(200, page("Permissions", body))
            if path == "/slow":
                time.sleep(SLOW_SECONDS)
                return self.send(200, page("Slow page", "<h1>Slow page loaded</h1><p>It took a long while.</p>"))
            return self.send(404, page("Not found", "<h1>Not found</h1>"))

        def control(self, path, query):
            if path == "/__requests":
                with state.lock:
                    body = json.dumps(state.requests).encode()
                return self.send(200, body, "application/json")
            if path == "/__reset":
                with state.lock:
                    state.requests.clear()
                    state.connections.clear()
                return self.send(200, b"{}", "application/json")
            if path == "/__counts":
                with state.lock:
                    body = json.dumps(state.connections).encode()
                return self.send(200, body, "application/json")
            if path == "/__flaky":
                state.set_flaky(query.get("on", ["0"])[0] == "1")
                return self.send(200, b"{}", "application/json")
            return self.send(404, b"{}", "application/json")

    return Handler


class Site(ThreadingHTTPServer):
    daemon_threads = True
    allow_reuse_address = True

    def __init__(self, port, name, state, certificate=None):
        self.name = name
        self.state = state
        self.context = None
        if certificate is not None:
            self.context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            self.context.load_cert_chain(*certificate)
        super().__init__(("127.0.0.1", port), make_handler(name, state, self.context))

    def get_request(self):
        connection, address = self.socket.accept()
        self.state.count(self.name)
        if self.context is not None:
            connection = self.context.wrap_socket(connection, server_side=True, do_handshake_on_connect=False)
        return connection, address

    def handle_error(self, request, client_address):
        # A client that refuses the certificate or hangs up is part of the tests, not an error to print.
        if not isinstance(sys.exc_info()[1], (ssl.SSLError, OSError)):
            super().handle_error(request, client_address)


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--certificates", type=Path, required=True,
                        help="the folder with trusted.pem, trusted-key.pem, untrusted.pem and untrusted-key.pem")
    parser.add_argument("--host", default=EMULATOR_HOST, help="how the phone reaches this computer")
    args = parser.parse_args()

    trusted = (args.certificates / "trusted.pem", args.certificates / "trusted-key.pem")
    untrusted = (args.certificates / "untrusted.pem", args.certificates / "untrusted-key.pem")
    state = State()
    sites = {
        "trusted": Site(0, "trusted", state, trusted),
        "untrusted": Site(0, "untrusted", state, untrusted),
        "plain": Site(0, "plain", state),
    }
    flaky_port = free_port()

    state.make_flaky = lambda: Site(flaky_port, "flaky", state, trusted)
    for site in sites.values():
        threading.Thread(target=site.serve_forever, daemon=True).start()
    addresses = {
        "trusted": f"https://{args.host}:{sites['trusted'].server_address[1]}",
        "untrusted": f"https://{args.host}:{sites['untrusted'].server_address[1]}",
        "plain": f"http://{args.host}:{sites['plain'].server_address[1]}",
        "flaky": f"https://{args.host}:{flaky_port}",
    }
    print(json.dumps(addresses), flush=True)
    try:
        # Runs until it is stopped, or until whoever started it closes its input.
        sys.stdin.read()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
