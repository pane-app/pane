"""Research capture only. Requires the reference board open in use-browser.

Usage: python docs/research/ui-port/capture-reference.py <stable-tab-id>
Does not alter the source HTML, application, browser profile or desktop settings.
"""
import json
import subprocess
import sys
from pathlib import Path

OUT = Path(__file__).parent / "reference"
TAB = sys.argv[1]
NAMES = ["root", "actions", "calculator", "empty", "clipboard", "window-manager", "store", "settings", "design-language"]


def browser(command, *args, source=None):
    result = subprocess.run(
        ["use-browser", "--tab", TAB, command, *args], input=source,
        capture_output=True, text=True, encoding="utf-8", check=True,
    )
    return result.stdout.strip()


def evaluate(expression):
    return json.loads(browser("js", "-", source=expression))


frames = evaluate("JSON.stringify(Array.from(document.querySelectorAll('iframe'), f=>({title:f.title})))")
browser("cdp", "Page.bringToFront")
for index, (name, frame) in enumerate(zip(NAMES, frames)):
    browser("js", "-", source=f"document.querySelectorAll('iframe')[{index}].scrollIntoView({{block:'start'}})")
    browser("shot", str(OUT / (name + "-board.png")))
print("Captured", len(frames), "reference boards")
