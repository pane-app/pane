"""A stand-in for Pane's releases on GitHub, for the native smokes: serves,
on 127.0.0.1 only, the release assets in a folder (target/dist/release,
where `cargo xtask package-linux`, `cargo xtask package-windows` or
`cargo xtask package-macos` puts the package of the version it built and
its .sha256) as GitHub serves Pane's releases: the listing GitHub's API
gives (/repos/pane-app/pane/releases), one release `v<version>` for each
version a package in the folder names, and each asset's download
(/pane-app/pane/releases/download/v<version>/<name>). The listing is read
from the folder on every request, so a package changed on disk is what
is served next. Nothing reaches the network or Pane's published releases.

Usage: release_server.py <release-folder> <port-file>
It listens on a free port, writes it to <port-file> once it is listening
and serves until it is stopped. Point the development build at it with
PANE_RELEASES=http://127.0.0.1:<port>/.
"""
import http.server
import json
import os
import re
import socketserver
import sys

folder, port_file = sys.argv[1], sys.argv[2]

# A package as `cargo xtask package-*` names a release's:
# pane-<version>-<os>-<arch>.zip or .tar.gz.
PACKAGE = re.compile(r"^pane-([0-9]+(?:\.[0-9]+)*)-[a-z0-9]+-[a-z0-9_]+\.(?:zip|tar\.gz)$")
LISTING = "/repos/pane-app/pane/releases"
DOWNLOADS = "/pane-app/pane/releases/download/"


def packages():
    return sorted(name for name in os.listdir(folder) if PACKAGE.match(name))


def version_of(name):
    """The version the package `name` (or its .sha256) is of, or None."""
    if name.endswith(".sha256"):
        name = name[: -len(".sha256")]
    match = PACKAGE.match(name)
    return match.group(1) if match else None


if not packages():
    sys.exit("release_server.py: %s holds no package; run `cargo xtask package-linux`, `cargo xtask package-windows` or `cargo xtask package-macos`" % folder)


class Releases(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        path = self.path.split("?")[0]
        if path == LISTING:
            return self.answer(200, json.dumps(self.listing()).encode(), "application/json")
        if path.startswith(DOWNLOADS):
            parts = path[len(DOWNLOADS):].split("/")
            if len(parts) == 2 and parts[0].startswith("v"):
                tag, name = parts
                asset = os.path.join(folder, name)
                if version_of(name) == tag[1:] and os.path.isfile(asset):
                    with open(asset, "rb") as f:
                        return self.answer(200, f.read())
        self.answer(404, b"not found")

    def listing(self):
        """The releases, newest first, as GitHub's API lists them (the
        fields Pane reads, and a few it does not)."""
        host = "http://%s:%d" % self.server.server_address[:2]
        releases = {}
        for name in packages():
            version = version_of(name)
            assets = releases.setdefault(version, [])
            for asset in (name, name + ".sha256"):
                path = os.path.join(folder, asset)
                if os.path.isfile(path):
                    assets.append({
                        "name": asset,
                        "size": os.path.getsize(path),
                        "state": "uploaded",
                        "browser_download_url": "%s%sv%s/%s" % (host, DOWNLOADS, version, asset),
                    })
        newest_first = sorted(releases, key=lambda v: [int(n) for n in v.split(".")], reverse=True)
        return [{
            "tag_name": "v" + version,
            "name": "Pane " + version,
            "draft": False,
            "prerelease": True,
            "assets": releases[version],
        } for version in newest_first]

    def answer(self, status, body, kind="application/octet-stream"):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        sys.stderr.write("release server: " + (format % args) + "\n")


class Server(http.server.ThreadingHTTPServer):
    """ThreadingHTTPServer without its reverse DNS lookup: HTTPServer's own
    server_bind names the server with socket.getfqdn of its address before
    it listens, and that lookup can block for many seconds on some hosts
    (as reported for macOS CI runners). The name is never used."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


server = Server(("127.0.0.1", 0), Releases)
with open(port_file + ".tmp", "w") as f:
    f.write(str(server.server_address[1]))
os.replace(port_file + ".tmp", port_file)
server.serve_forever()
