"""Builds the grouped history from history-groups.json into the main
checkout as branch `history-grouped` (local only; pushes nothing).
Each group becomes one commit with the tree of its last commit."""
import json, os, re, subprocess, sys

REPO = r"C:\Users\ADMIN\Desktop\nguyenvu\pane"
MIRROR = os.path.join(os.environ["TEMP"], "pane-history-backup.git")
PLAN = os.path.join(os.environ["TEMP"], "pane-wave", "history-groups.json")
SIGN = "hoangvu12 <hggaming91@gmail.com>"


def git(*args, env=None, stdin=None):
    return subprocess.run(["git", "-C", REPO, *args], check=True, capture_output=True,
                          text=True, encoding="utf-8", env=env, input=stdin).stdout.strip()


subprocess.run(["git", "-C", REPO, "fetch", "-q", MIRROR, "+refs/heads/main:refs/old-main"], check=True)
groups = json.load(open(PLAN, encoding="utf-8"))
parent, mapping = None, {}
for g in groups:
    name, email = re.match(r"(.*) <(.*)>", g["author"]).groups()
    body = g["body"].strip()
    trailers = [f"Co-authored-by: {c}" for c in g.get("coauthors", [])] + [f"Signed-off-by: {SIGN}"]
    msg = f"{g['subject']}\n\n{body}\n\n" + "\n".join(trailers) + "\n"
    env = dict(os.environ, GIT_AUTHOR_NAME=name, GIT_AUTHOR_EMAIL=email, GIT_AUTHOR_DATE=g["date"],
               GIT_COMMITTER_NAME="hoangvu12", GIT_COMMITTER_EMAIL="hggaming91@gmail.com",
               GIT_COMMITTER_DATE=g["date"])
    args = ["commit-tree", g["last"] + "^{tree}", "-F", "-"] + (["-p", parent] if parent else [])
    parent = git(*args, env=env, stdin=msg)
    mapping[g["last"]] = parent
git("branch", "-f", "history-grouped", parent)
git("update-ref", "-d", "refs/old-main")

ok = True
for label, old in (("main", "f95529fa35fa4a765f4df186e58232b6c6143a67"), ("v0.1.0", "989a8021285f7606fa771bbc61c7773687e4e287")):
    full = next(k for k in mapping if k.startswith(old[:8]))
    same = git("rev-parse", full + "^{tree}") == git("rev-parse", mapping[full] + "^{tree}")
    ok &= same
    print(f"{label}: old {old[:8]} -> new {mapping[full][:8]}  tree match: {same}")
print("commits:", git("rev-list", "--count", "history-grouped"))
json.dump({k[:8]: v for k, v in mapping.items()}, open(PLAN.replace(".json", "-map.json"), "w"))
sys.exit(0 if ok else 1)
