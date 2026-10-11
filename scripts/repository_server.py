"""Git repositories for the native smokes, served over Git's smart HTTP
protocol from 127.0.0.1 only; nothing reaches the network.

Usage:
  repository_server.py make-sample <sample-folder> <repository-folder>
      Makes the controlled repository of the Git sample that
      `cargo xtask guests` assembles (target/guests/git/greeter): its source
      (everything but dist/) committed on `main`, then the branch `release`
      adding the built component under dist/, tagged `v0.1.0`.
  repository_server.py clone-defaults <committed-pins> <repositories-folder> <pins-file> <url>
      Clones the default extensions' repositories at the commits the
      committed pins file names (crates/pane/defaults.json) into
      <repositories-folder> — the smoke's own setup, from their real
      addresses on GitHub; the Pane under test fetches only from <url> —
      and writes <pins-file>: the pins a development build's PANE_DEFAULTS
      names, the same ids, titles, tags and commits as the committed pins,
      pointing each at <url><id>.git. An entry that names a platform is
      written through, so a Pane whose system is not that one never
      fetches it. Run it after `serve` has written its
      port, with <url> the address it serves at. The clones hold the
      release revisions' built components, so a first setup installs
      exactly what a release installs.
  repository_server.py commit <repository-folder> <reference>
      Prints the id of the commit <reference> (such as v0.1.0) points to, to
      check the one Pane records.
  repository_server.py move-sample <repository-folder> <version>
      On the branch `release`, commits a new release of the sample at
      <version> (its pane.json's version field), leaving `main` and the
      tags where they were: a tracked branch moving, as the smoke's
      automatic-update phase needs.
  repository_server.py serve <repositories-folder> <port-file>
      Serves each repository in the folder as /<name>.git on a free port,
      writes the port to <port-file> once listening, and serves until it is
      stopped. Each request runs `git upload-pack --stateless-rpc`, as
      `git http-backend` does.

Only this script runs `git`, and with none of the user's configuration.
Pane itself never runs `git`.
"""
import http.server
import json
import os
import shutil
import socketserver
import subprocess
import sys


def git_env(home):
    env = {key: os.environ[key] for key in ("PATH", "SYSTEMROOT", "TMP", "TEMP", "TMPDIR") if key in os.environ}
    env.update({
        "HOME": home,
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.path.join(home, "gitconfig"),
        "GIT_AUTHOR_NAME": "Pane smoke",
        "GIT_AUTHOR_EMAIL": "smoke@pane.invalid",
        "GIT_COMMITTER_NAME": "Pane smoke",
        "GIT_COMMITTER_EMAIL": "smoke@pane.invalid",
        "GIT_AUTHOR_DATE": "2026-09-29T12:00:00Z",
        "GIT_COMMITTER_DATE": "2026-09-29T12:00:00Z",
    })
    return env


def make_sample(sample, repository):
    home = os.path.join(os.path.dirname(os.path.abspath(repository)), ".home")
    os.makedirs(home, exist_ok=True)
    if os.path.exists(repository):
        shutil.rmtree(repository)
    os.makedirs(repository)
    env = git_env(home)

    def git(*args):
        subprocess.run(["git", *args], cwd=repository, env=env, check=True, stdout=subprocess.DEVNULL)

    def copy(include_dist):
        for root, _, files in os.walk(sample):
            relative = os.path.relpath(root, sample)
            if not include_dist and relative.split(os.sep)[0] == "dist":
                continue
            os.makedirs(os.path.join(repository, relative), exist_ok=True)
            for name in files:
                shutil.copyfile(os.path.join(root, name), os.path.join(repository, relative, name))

    git("init", "--quiet", "--initial-branch=main")
    copy(False)
    git("add", "--all")
    git("commit", "--quiet", "-m", "Greeter 0.1.0 source")
    git("switch", "--quiet", "-c", "release")
    copy(True)
    git("add", "--all", "--force")
    git("commit", "--quiet", "-m", "Release 0.1.0")
    git("tag", "--annotate", "-m", "v0.1.0", "v0.1.0")
    git("switch", "--quiet", "main")


def commit(repository, reference):
    home = os.path.join(os.path.dirname(os.path.abspath(repository)), ".home")
    result = subprocess.run(
        ["git", "rev-parse", "--verify", reference + "^{commit}"],
        cwd=repository, env=git_env(home), check=True, capture_output=True, text=True,
    )
    print(result.stdout.strip())


def serve(folder, port_file):
    home = os.path.join(os.path.abspath(folder), ".home")
    os.makedirs(home, exist_ok=True)
    env = git_env(home)

    class Server(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def repository(self, suffix):
            path = self.path.split("?", 1)[0].lstrip("/")
            if not path.endswith(suffix):
                return None
            name = path[: -len(suffix)]
            name = name[:-4] if name.endswith(".git") else name
            directory = os.path.join(folder, name)
            if "/" in name or "\\" in name or name.startswith(".") or not os.path.isdir(directory):
                return None
            return directory

        def upload_pack(self, directory, advertise, body):
            run_env = dict(env)
            if "version=2" in (self.headers.get("Git-Protocol") or ""):
                run_env["GIT_PROTOCOL"] = "version=2"
            command = ["git", "upload-pack", "--stateless-rpc"]
            if advertise:
                command.append("--advertise-refs")
            command.append(directory)
            result = subprocess.run(command, input=body, capture_output=True, env=run_env)
            return result.stdout

        def do_GET(self):
            directory = self.repository("/info/refs")
            if directory is None or not self.path.endswith("?service=git-upload-pack"):
                return self.answer(404, b"", "text/plain")
            out = self.upload_pack(directory, True, b"")
            self.answer(200, out, "application/x-git-upload-pack-advertisement")

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length)
            directory = self.repository("/git-upload-pack")
            if directory is None:
                return self.answer(404, b"", "text/plain")
            out = self.upload_pack(directory, False, body)
            self.answer(200, out, "application/x-git-upload-pack-result")

        def answer(self, status, body, content_type):
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Connection", "close")
            self.end_headers()
            self.wfile.write(body)
            self.close_connection = True

        def log_message(self, format, *args):
            sys.stderr.write("repository server: " + (format % args) + "\n")

    server = Listener(("127.0.0.1", 0), Server)
    with open(port_file + ".tmp", "w") as f:
        f.write(str(server.server_address[1]))
    os.replace(port_file + ".tmp", port_file)
    server.serve_forever()


class Listener(http.server.ThreadingHTTPServer):
    """ThreadingHTTPServer without its reverse DNS lookup: HTTPServer's own
    server_bind names the server with socket.getfqdn of its address before
    it listens, and that lookup can block for many seconds on some hosts
    (as reported for macOS CI runners). The name is never used."""

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


def move_sample(repository, version):
    """On the branch `release`, commits a new release of the sample at
    <version> (its pane.json's version field), leaving `main` and the tags
    where they were: a tracked branch moving, as the smoke's automatic
    update phase needs."""
    home = os.path.join(os.path.dirname(os.path.abspath(repository)), ".home")
    env = git_env(home)

    def git(*args):
        subprocess.run(["git", *args], cwd=repository, env=env, check=True, stdout=subprocess.DEVNULL)

    git("switch", "--quiet", "release")
    manifest = os.path.join(repository, "pane.json")
    with open(manifest, encoding="utf-8") as f:
        text = f.read()
    replaced = text.replace('"version": "0.1.0"', '"version": "%s"' % version)
    if replaced == text:
        sys.exit("the sample's pane.json has no 0.1.0 version to move")
    with open(manifest, "w", encoding="utf-8") as f:
        f.write(replaced)
    git("add", "--all")
    git("commit", "--quiet", "-m", "Release %s" % version)
    git("switch", "--quiet", "main")


def clone_defaults(committed, repositories, pins_file, url):
    """Clones the default extensions' repositories at the commits the
    committed pins file names (crates/pane/defaults.json) — their real
    addresses, as the smoke's own setup on the runner; the Pane under test
    fetches only from the URL it is served at — and writes the pins file
    naming them: a JSON array of { id, title, repository, tag, commit,
    platform } pointing each at the URL the smoke serves them from, with
    the same ids, titles, tags and commits as the committed pins, and the
    same platform: an entry that names one is served as it is committed,
    so a Pane whose system is not that one never fetches it."""
    with open(committed, encoding="utf-8") as f:
        committed = json.load(f)
    pins = []
    for pin in committed:
        id = pin["id"]
        repository = os.path.join(repositories, id)
        if os.path.exists(repository):
            shutil.rmtree(repository)
        home = os.path.join(os.path.dirname(os.path.abspath(repository)), ".home")
        os.makedirs(home, exist_ok=True)
        env = git_env(home)
        # The clone is smoke setup on the runner: it reaches the real
        # repository. The commit is checked out to prove the clone holds
        # it; serving needs only the objects.
        subprocess.run(["git", "clone", "--quiet", pin["repository"], repository],
                       env=env, check=True)
        subprocess.run(["git", "checkout", "--quiet", "--detach", pin["commit"]],
                       cwd=repository, env=env, check=True)
        served = {
            "id": id,
            "title": pin["title"],
            "repository": "%s%s.git" % (url, id),
            "tag": pin["tag"],
            "commit": pin["commit"],
        }
        if "platform" in pin:
            served["platform"] = pin["platform"]
        pins.append(served)
    with open(pins_file + ".tmp", "w", encoding="utf-8") as f:
        json.dump(pins, f, indent=2)
    os.replace(pins_file + ".tmp", pins_file)


if __name__ == "__main__":
    if sys.argv[1:2] == ["make-sample"] and len(sys.argv) == 4:
        make_sample(sys.argv[2], sys.argv[3])
    elif sys.argv[1:2] == ["clone-defaults"] and len(sys.argv) == 6:
        clone_defaults(sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5])
    elif sys.argv[1:2] == ["commit"] and len(sys.argv) == 4:
        commit(sys.argv[2], sys.argv[3])
    elif sys.argv[1:2] == ["move-sample"] and len(sys.argv) == 4:
        move_sample(sys.argv[2], sys.argv[3])
    elif sys.argv[1:2] == ["serve"] and len(sys.argv) == 4:
        serve(sys.argv[2], sys.argv[3])
    else:
        sys.exit(__doc__)
