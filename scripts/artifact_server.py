"""A local artifact source for the native smokes: serves, on 127.0.0.1
only, the files in a folder (such as target/dist/artifacts, where
`cargo xtask package-linux`, `cargo xtask package-windows` or
`cargo xtask package-macos` assembles the index of Pane's own
application updates and the application package it names) as Pane's own
downloads do: the index document pane-defaults.json and the package
file. Nothing reaches the network or Pane's published downloads.

Usage: artifact_server.py <artifact-folder> <port-file>
It listens on a free port, writes it to <port-file> once it is listening
and serves until it is stopped. Point the development build at it with
PANE_ARTIFACTS=http://127.0.0.1:<port>/.
"""
import http.server
import os
import socketserver
import sys

folder, port_file = sys.argv[1], sys.argv[2]

if not os.path.isfile(os.path.join(folder, "pane-defaults.json")):
    sys.exit("artifact_server.py: %s holds no pane-defaults.json; run `cargo xtask package-linux`, `cargo xtask package-windows` or `cargo xtask package-macos`" % folder)

TYPES = {
    ".json": "application/json",
    ".tgz": "application/octet-stream",
}


class Artifacts(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        name = self.path.lstrip("/").split("?")[0]
        path = os.path.join(folder, name)
        if name != os.path.basename(name) or not os.path.isfile(path):
            return self.answer(404, b"not found")
        with open(path, "rb") as f:
            body = f.read()
        kind = TYPES.get(os.path.splitext(name)[1], "application/octet-stream")
        self.answer(200, body, kind)

    def answer(self, status, body, kind="text/plain"):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        sys.stderr.write("artifact server: " + (format % args) + "\n")


class Server(http.server.ThreadingHTTPServer):
    """ThreadingHTTPServer without its reverse DNS lookup: HTTPServer's own
    server_bind names the server with socket.getfqdn of its address before
    it listens, and that lookup can block for many seconds on some hosts
    (as reported for macOS CI runners). The name is never used."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


server = Server(("127.0.0.1", 0), Artifacts)
with open(port_file + ".tmp", "w") as f:
    f.write(str(server.server_address[1]))
os.replace(port_file + ".tmp", port_file)
server.serve_forever()
