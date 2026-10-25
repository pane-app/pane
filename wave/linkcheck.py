"""Check relative links and docs/ path mentions in a git checkout.

Usage: python -I linkcheck.py <repo>

1. Every Markdown link `[x](target)`, reference definition `[x]: target`
   and HTML `src=`/`href=` attribute in tracked *.md files whose target is
   relative (not a URL, not a bare #anchor) must resolve to an existing
   file or folder.
2. Every `docs/...` path mentioned in any tracked file (code, CI, scripts,
   TOML, Markdown) that is not part of a URL must resolve from the repo
   root (or from the mentioning file's folder).
"""
import os
import re
import subprocess
import sys
from urllib.parse import unquote

NL = chr(10)
repo = os.path.abspath(sys.argv[1])
files = subprocess.run(
    ["git", "-C", repo, "ls-files", "-co", "--exclude-standard"],
    capture_output=True, text=True, check=True,
).stdout.splitlines()
files = [f for f in files if os.path.isfile(os.path.join(repo, f))]

MD_LINK = re.compile(r"\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
MD_REF = re.compile(r"^\s*\[[^\]]+\]:\s*<?(\S+?)>?(?:\s|$)", re.M)
HTML_ATTR = re.compile(r"(?:src|href)=\"([^\"]+)\"")
DOCS_PATH = re.compile(r"(?<![\w./:-])(docs/[\w./@+-]*[\w/])")
SKIP_EXT = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".ico", ".icns", ".wasm",
            ".lock", ".bin", ".zip", ".ttf", ".otf", ".woff", ".woff2", ".exe"}

def is_external(t):
    return re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", t) or t.startswith("//")

def exists(base_dir, target):
    t = unquote(target.split("#", 1)[0].split("?", 1)[0])
    if not t:
        return True
    if t.startswith("/"):
        p = os.path.join(repo, t.lstrip("/"))
    else:
        p = os.path.normpath(os.path.join(base_dir, t))
    return os.path.exists(p)

md_checked = docs_checked = 0
broken = []
for f in files:
    if os.path.splitext(f)[1].lower() in SKIP_EXT:
        continue
    full = os.path.join(repo, f)
    try:
        text = open(full, encoding="utf-8").read()
    except (UnicodeDecodeError, OSError):
        continue
    base = os.path.dirname(full)
    if f.endswith(".md"):
        # Ignore fenced code blocks.
        stripped = re.sub(r"```.*?```", lambda m: NL * m.group(0).count(NL), text, flags=re.S)
        for rx in (MD_LINK, MD_REF, HTML_ATTR):
            for m in rx.finditer(stripped):
                t = m.group(1)
                if is_external(t) or t.startswith("#") or t.startswith("@"):
                    continue
                md_checked += 1
                if not exists(base, t):
                    line = stripped[: m.start()].count("\n") + 1
                    broken.append(("md-link", f, line, t))
    for m in DOCS_PATH.finditer(text):
        t = m.group(1).rstrip(".")
        # Glob-ish or placeholder mentions are not paths.
        if any(c in t for c in "*<>{}"):
            continue
        docs_checked += 1
        if not (os.path.exists(os.path.join(repo, t)) or os.path.exists(os.path.join(base, t))):
            line = text[: m.start()].count("\n") + 1
            broken.append(("docs-path", f, line, t))

print(f"files scanned: {len(files)}; markdown links checked: {md_checked}; docs/ mentions checked: {docs_checked}")
for kind, f, line, t in broken:
    print(f"BROKEN {kind}: {f}:{line} -> {t}")
print(f"broken: {len(broken)}")
