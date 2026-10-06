"""Unpack the retained reference as research sources; never modify the input.

python docs/research/ui-port/extract-reference.py
Font metadata additionally uses the locally installed fontTools package.
"""
import base64
import gzip
import hashlib
import io
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).parent / "reference"
SOURCE = ROOT / "docs/evidence/ui-prototype/reference/launcher.html"
EXPECTED = "f7e81e030e2216fe61509b9afd98a68147d1998c73f0a168bab02d0bf00f0bb4"
NAMES = ["root", "actions", "calculator", "empty", "clipboard", "window-manager", "store", "settings", "design-language"]


def payload(document, name):
    match = re.search(r'<script type="__bundler/' + name + r'">(.*?)</script>', document, re.S)
    return json.loads(match.group(1))


def decode(item):
    data = base64.b64decode(item["data"])
    return gzip.decompress(data) if item.get("compressed") else data


raw = SOURCE.read_bytes()
assert hashlib.sha256(raw).hexdigest() == EXPECTED, "Reference changed; review the new authority first"
document = raw.decode("utf-8")
manifest = payload(document, "manifest")
pages = payload(document, "page_order")
assert len(pages) == len(NAMES)
fonts = {}
for name, page in zip(NAMES, pages):
    nested = decode(manifest[page]).decode("utf-8")
    template = payload(nested, "template")
    styles = re.findall(r'<style[^>]*>(.*?)</style>', template, re.S)
    authored = "\n".join(s for s in styles if "@font-face" not in s)
    logic = re.findall(r'<script type="text/x-dc"[^>]*>(.*?)</script>', template, re.S)
    markup = re.search(r'<x-dc[^>]*>(.*?)</x-dc>', template, re.S).group(1)
    markup = re.sub(r'<helmet>.*?</helmet>', '', markup, flags=re.S)
    (OUT / (name + ".css")).write_text(authored.strip() + "\n", encoding="utf-8")
    (OUT / (name + ".js")).write_text("\n".join(logic).strip() + "\n", encoding="utf-8")
    (OUT / (name + "-template.html")).write_text(markup.strip() + "\n", encoding="utf-8")
    for asset_id, asset in payload(nested, "manifest").items():
        if asset["mime"].startswith("font/") and asset_id not in fonts:
            from fontTools.ttLib import TTFont
            font_bytes = decode(asset)
            font = TTFont(io.BytesIO(font_bytes))
            fonts[asset_id] = {
                "sha256": hashlib.sha256(font_bytes).hexdigest(),
                "names": {str(i): font["name"].getDebugName(i) for i in [1, 2, 5, 6]},
                "axes": [{"tag": a.axisTag, "min": a.minValue, "default": a.defaultValue, "max": a.maxValue} for a in font["fvar"].axes] if "fvar" in font else [],
            }
(OUT / "font-metadata.json").write_text(json.dumps(fonts, indent=2) + "\n", encoding="utf-8")
print("Extracted 9 complete authored templates, CSS and logic; identified", len(fonts), "fonts")
