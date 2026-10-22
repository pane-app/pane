"""A local npm registry for the native smokes: serves, on 127.0.0.1 only,
the npm tarballs in a folder (such as target/guests/npm, where
`cargo xtask guests` packs the npm sample) as npm's registry does: each
package's abbreviated metadata, with its sha512 integrity, at /<name> (a
scoped name's `/` written %2f), and each tarball at /<name>/-/<file>. Every
tarball is the latest version of its package. The folder is read again on
request, so a smoke can publish a newer version into it while Pane runs
(or, as the update phase does, while Pane is stopped, to be found by the
check at its next start). Nothing reaches the network.

Usage: npm_registry.py <tarball-folder> <port-file>
It listens on a free port, writes it to <port-file> once it is listening and
serves until it is stopped.
"""
import base64
import hashlib
import http.server
import json
import os
import socketserver
import sys
import tarfile
import threading

folder, port_file = sys.argv[1], sys.argv[2]

scan_lock = threading.Lock()


def scan():
    """The packages the folder holds now: each tarball's name and version,
    and which version is each package's latest (the highest one, as the
    last in sorted order)."""
    packages = {}  # name -> {"latest": version, "versions": {version: (file, bytes)}}
    with scan_lock:
        for file in sorted(os.listdir(folder)):
            if not file.endswith(".tgz"):
                continue
            path = os.path.join(folder, file)
            try:
                with tarfile.open(path, "r:gz") as tar:
                    manifest = json.load(tar.extractfile("package/package.json"))
            except (OSError, tarfile.TarError, json.JSONDecodeError, KeyError):
                continue  # a tarball still being written
            name, version = manifest["name"], manifest["version"]
            with open(path, "rb") as f:
                tarball = f.read()
            entry = packages.setdefault(name, {"latest": version, "versions": {}})
            entry["versions"][version] = (name.rsplit("/", 1)[-1] + "-" + version + ".tgz", tarball)
            entry["latest"] = version
    return packages


class Registry(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        base = "http://127.0.0.1:%d/" % self.server.server_address[1]
        path = self.path.lstrip("/")
        packages = scan()
        if "/-/" in path:
            name, file = path.split("/-/", 1)
            versions = packages.get(name, {}).get("versions", {})
            found = [tarball for (f, tarball) in versions.values() if f == file]
            return self.answer(200, found[0]) if found else self.answer(404, b"{}")
        name = path.replace("%2f", "/").replace("%2F", "/")
        if name not in packages:
            return self.answer(404, b'{"error":"Not found"}')
        entry = packages[name]
        versions = {}
        for version, (file, tarball) in entry["versions"].items():
            digest = base64.b64encode(hashlib.sha512(tarball).digest()).decode()
            versions[version] = {
                "name": name,
                "version": version,
                "dist": {
                    "tarball": base + name + "/-/" + file,
                    "integrity": "sha512-" + digest,
                },
            }
        metadata = {"name": name, "dist-tags": {"latest": entry["latest"]}, "versions": versions}
        self.answer(200, json.dumps(metadata).encode())

    def answer(self, status, body):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        sys.stderr.write("npm registry: " + (format % args) + "\n")


class Server(http.server.ThreadingHTTPServer):
    """ThreadingHTTPServer without its reverse DNS lookup: HTTPServer's own
    server_bind names the server with socket.getfqdn of its address before
    it listens, and that lookup can block for many seconds on some hosts
    (as reported for macOS CI runners). The name is never used."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


server = Server(("127.0.0.1", 0), Registry)
with open(port_file + ".tmp", "w") as f:
    f.write(str(server.server_address[1]))
os.replace(port_file + ".tmp", port_file)
server.serve_forever()
