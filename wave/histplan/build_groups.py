"""Build and validate the first-parent grouping plan for old pane main.

Usage: python -I build_groups.py <mirror.git> <out-dir>
"""
import collections
import json
import os
import subprocess
import sys

MIRROR, OUT = sys.argv[1], sys.argv[2]
TIP = "f95529fa"
V010 = "989a8021"
HOANG = "hoangvu12 <hggaming91@gmail.com>"


def git(*args):
    return subprocess.run(["git", "-C", MIRROR, *args], check=True,
                          capture_output=True, text=True, encoding="utf-8").stdout


rows = git("log", "--first-parent", "--reverse", "--format=%H%x09%P%x09%aI%x09%an <%ae>", TIP).splitlines()
fp = []
for r in rows:
    h, parents, date, author = r.split("\t")
    fp.append({"sha": h, "parents": parents.split(), "date": date, "author": author})
assert len(fp) == 289, len(fp)


def norm(a):
    return HOANG if a.startswith("Vu Nguyen ") else a


def authors_of(c):
    if len(c["parents"]) > 1:
        out = git("log", "--format=%an <%ae>", f"{c['parents'][0]}..{c['sha']}").splitlines()
    else:
        out = [c["author"]]
    return [norm(a) for a in out]


# (first index, last index) 1-based along --first-parent, subject, body
G = [
 (1, 2, "Record the Pane specification and plan, and track the work in GitHub Issues",
  "Adds the Pane specification, the cross-platform plan and the extension\nruntime research, then publishes the first tickets and moves planning\nto GitHub Issues."),
 (3, 7, "Run the first Rust and JS/TS commands in Pane's native window",
  "Runs one Rust command in Pane's native window, validates the JS/TS\nWASI 0.3 backend and runs JS and TS versions of the sample command.\nApplies the licensing split and DCO, and records native Linux, macOS\nand Windows baselines with CI evidence.\n\nPRs: #2, #3, #5, #6, #7, #8"),
 (8, 11, "Install local extension packages and show their forms and custom views",
  "Installs and runs a local extension package, submits and validates\nnative forms, declares platform availability and keeps the selected row\nin view. Disabling an extension keeps its settings, and a custom view\ncan choose a color.\n\nPRs: #9, #10, #19, #20, #21"),
 (12, 15, "Find commands, calculator answers and quicklinks in root search",
  "Typing in root search finds and invokes commands, shows a calculator\nresult and opens persistent URL quicklinks. Extensions can call the\noperations another installed extension publishes.\n\nPRs: #22, #23, #27, #28"),
 (16, 21, "Find applications, open commands by global hotkey and manage the extension lifecycle",
  "Root search finds and opens installed applications, and a global hotkey\nopens an extension command on Windows, macOS and Linux (X11 grabs,\nraised window). Extensions can be reloaded and their cache cleared,\npending calls stop on disable or reload, and uninstall asks what to do\nwith saved data.\n\nPRs: #11, #14, #24, #25, #26, #32, #33, #34, #39, #40"),
 (22, 29, "Develop local packages with reload on save, and manage dependencies",
  "Development mode builds, stages and reloads a local package on save,\nkills its processes and watches folders by identity. Extensions that\ncannot start are paused with Retry, retained data can be deleted, and\ncommands run through aliases and fallbacks. Required dependencies and\npackaged native helpers install, and dependents disable together.\n\nPRs: #12, #13, #15, #16, #31, #41, #42, #43"),
 (30, 34, "Keep Pane responsive when a guest crashes or hangs, and search files and the web",
  "Pane stays usable when its runtime thread crashes, and a watchdog stops\nunresponsive guests and stuck runtime threads, counting only a guest's\nown computing. Dependents uninstall together, and commands can find\nfiles in a chosen folder and search an online service.\n\nPRs: #17, #18, #29, #30, #44"),
 (35, 40, "Install extensions from npm and Git, and keep opt-in clipboard history",
  "Extension packages install from npm and Git repositories. Clipboard\nhistory is opt-in, watched on Windows, expires and can be deleted. The\nwatchdog charges no web response or nested host call to a guest and is\nbounded by the thread's own time.\n\nPRs: #18, #35, #36, #45, #46"),
 (41, 47, "Install Pane on every platform and run extension tasks and services",
  "Pane installs on Linux, Windows and macOS and acquires its default\nextensions. Extensions can run scheduled tasks and continuing services\nthat stop on disable, and clipboard history is watched on Linux and\nmacOS too.\n\nPRs: #37, #38, #47, #48, #51, #52, #53"),
 (48, 51, "Offer Pane and extension updates on every platform",
  "Pane offers application updates by user choice on Windows, Linux and\nmacOS. npm extensions update at a safe boundary, and tracked Git\nextensions update without changing identity.\n\nPRs: #49, #50, #54, #55, #56"),
 (52, 55, "Measure resource use and choose the release's default extensions",
  "Measures Pane's per-platform resource use and sets its targets, steadies\nthe smokes, keeps the launcher's Developing Arc so automatic updates run,\nand makes calculator, applications, quicklinks, files and clipboard\nhistory the release's defaults.\n\nPRs: #4, #58, #59, #60"),
 (56, 62, "Integrate the launcher UI on the maintained GPUI CE fork",
  "Separates launcher presentation ownership, pins the maintained GPUI CE\nfork with renderer regressions in CI, and applies shared dark and light\nvisuals with a bounded status footer. Records the Windows glass\nevidence, centralizes smoke color roles and rounds the Windows window.\n\nPRs: #61, #62, #63, #64"),
 (63, 67, "Open an independent Settings window and add command transitions",
  "Records the Settings and motion research, replaces the footer hints\nwith the selected primary action, opens Settings in its own window and\nadds subtle command open and back transitions. Adds a fast CI tier for\nmilestone worktrees.\n\nPRs: #70, #71, #72, #86"),
 (68, 74, "Fill the Settings pages for shortcuts, appearance, extensions and General",
  "Settings can browse commands and edit aliases, change and persist the\nappearance, manage extensions, record the Open Pane hotkey, choose\nlaunch at login and search its pages. The release matrix runs on main\nonly, and the CI tiers are documented.\n\nPRs: #73, #74, #75, #80, #81, #83"),
 (75, 82, "Finish the Settings pages and the shared motion policy",
  "Adds section and group transitions, the About page's updates and\ndiagnostics, the Launcher page's placement choices, Keyboard page\nrebinding, the tray or menu-bar entry, command hotkey recording, the\nsearchable select and the popup, hover and press micro-transitions.\n\nPRs: #76, #77, #78, #79, #82, #85, #87, #88"),
 (83, 91, "Port the launcher and Settings to the reference design",
  "Sets up a Windows reference/native comparison workbench, then matches\nthe 760x518 frame, keycaps, tiles, root rows, footer and Actions panel,\nthe Settings shell, no-results card, clipboard split view, pinned home,\nAppearance controls and shared controls, and records the acceptance\nevidence.\n\nPRs: #91, #92, #93, #94, #95, #96, #97, #98, #99, #101, #102, #103"),
 (92, 101, "Redesign the launcher and Settings in the Raycast style",
  "Checks in the UI port's research, draws glyphs from the vendored reicon\nset and ports the launcher and Settings window to the Raycast-style\nredesign. Adds a user-chosen background image that lets the frost show\nthrough, quicker view arrival, keyboard opens that land at once and\ncontrols that press on pointer-down."),
 (102, 110, "Bring docs and tests in line with the redesign and deepen the core modules",
  "Updates the docs and glossary, stops CI retries landing as commits,\ndeepens pane-core's launcher, downloads and test support, gives host\nsettings one commit path with rollback, separates window presence from\nLauncherWindow, covers the redesign with window tests and builds each\ncrate's integration tests as one binary.\n\nPRs: #106, #109, #110, #111, #112, #113, #118"),
 (111, 111, "Give extension commands the full command model",
  "Serves other extensions' calls while one waits, caps guest memory at\n128 MiB, carries lists through a typed envelope with several actions per\nitem, runs no-view commands, and adds icons, accessories, submenus,\ntoasts, host functions, preferences, arguments and system programs.\n\nPRs: #120, #134, #135, #136, #160"),
 (112, 115, "Complete default extensions, application discovery and indexed file search",
  "Gives applications stable identities, discovery, watching, localized\nnames and icons; indexes the home folder with Pane's own engine and a\nRaycast-like Search Files; keeps images and files in clipboard history;\nmanages extensions per page in Settings and draws only visible rows.\n\nPRs: #161-#176, #178, #179, #180, #181"),
 (116, 118, "Move to pane-app/pane and record how extensions are sourced and built",
  "Points live references at pane-app/pane after the repository moved, and\nrecords where extensions come from, how their API changes and how\npeople build them.\n\nPRs: #209, #210, #211"),
 (119, 126, "Keep extension logs and make the SDKs and build tooling publishable",
  "Keeps each extension's log and shows it on a Logs screen, builds the\npatched componentizer in CI for every platform, moves development\nbuilds into a shared build crate and makes the SDKs publishable as\npane-extension and @pane-app/extension. Records the Raycast research.\n\nPRs: #119, #212, #213, #215, #216, #220, #227, #228"),
 (127, 129, "Add pane-ext dev and the local channel, and land the quick fixes",
  "pane-ext dev drives the running Pane over a local channel, with its\nfailures in the log. The quick fixes bring the tray icon back after\nExplorer restarts, protect local secrets with DPAPI, announce the\nselected row, and keep a redacted log that reports unexpected quits.\n\nPRs: #129, #130, #131, #132, #133, #182, #217, #257, #271, #272"),
 (130, 134, "Meet the file-index and idle-cost targets and record the evidence",
  "File queries answer within 10 ms at p95, changes re-index within 10 ms\nand 10,000 changes catch up within 1 s. While hidden, Pane re-extracts\nicons only when needed, lets its runtime timers sleep and shares\nclipboard history with its view. Records the measurements.\n\nPRs: #126, #183-#192, #273, #274, #275, #277, #288"),
 (135, 137, "Install the official extensions from pinned repositories",
  "First setup installs the default extensions from the commits this\nrelease pins, Settings marks them official, and their sources leave\nPane's repository with samples kept as fixtures. Smokes serve the\npinned repositories locally.\n\nPRs: #207, #278, #280, #282, #284, #285, #286, #291, #296, #297"),
 (138, 139, "Check for extension updates on demand and record their results",
  "Records update results, shows them and announces background failures,\nchecks for extension updates on demand and updates all, and updates\ndefault extensions from their repositories' release tags.\n\nPRs: #127, #256, #267, #269, #293, #298"),
 (140, 142, "Add Windows power features: paste, Run, System Commands, Switch Windows and game mode",
  "Pastes into the front application, adds Run with Win+R history, System\nCommands, Switch Windows and reading the selected text, falls back to a\nkeyboard hook, binds modifier taps and the Windows key alone, and adds\ngame mode. Fixes the release matrix after the merge and records the\nredesign's final state.\n\nPRs: #105, #125, #252-#255, #259-#266, #268, #292, #306, #315"),
 (143, 143, "Complete extension authoring: check, pack, templates and the CLI",
  "Adds a crash overlay with the stack trace, the pane.json schema and\npane-ext check, JS/TS builds in pane-build, pane-ext pack, pane-ext as\n@pane-app/cli, templates with pane-ext new and npm create @pane-app,\nand Create Extension and Import Extension in Pane.\n\nPRs: #128, #214, #218, #219, #221, #222, #223, #224, #225, #290"),
 (144, 145, "Polish the launcher's text, toasts, HUD, selection and native materials",
  "Sets text levels by alpha with contrast floors, draws named glyphs on a\nneutral tile, shows outcomes as toasts and a HUD, refines the selection\nwash and loading bar, fires chords only on exact modifiers, backs out\nwith Backspace, and picks macOS vibrancy and an opaque Linux launcher.\n\nPRs: #66, #67, #123, #245-#251, #258, #289, #313"),
 (146, 147, "Rank root search like Raycast, with fuzzy matching, learning and inline arguments",
  "Matches fuzzily and without accents, understands addresses and paths,\nranks with Raycast's comparator and keywords, publishes within 200 ms,\nbrowses typed folders, learns from choices with a reset, holds keys\nuntil the list is published, and adds inline arguments and recent\nqueries.\n\nPRs: #122, #193-#206, #295, #322"),
 (148, 148, "Let extensions provide and use capabilities and register items at run time",
  "Packages provide and use named capabilities routed through Pane, with\nthe provider chosen in Settings, shown in the install preview and in\nManage extensions. Extensions register root items, timers, watchers\nand providers at run time and hand state to new code on reload.\n\nPRs: #151-#158, #276"),
 (149, 149, "Pin the Windows default extensions and gate defaults by platform",
  "Pins the Windows defaults in defaults.json gated by platform, serves\ntheir repositories in tests, exercises them in the smokes and moves\ntheir builds out of Pane's repository.\n\nPRs: #300, #302, #303, #304, #305, #314"),
 (150, 152, "Install, update and develop extensions from collections",
  "Installs one, some or all of a collection's extensions, updates them\nfrom their own release tags, develops one from a collection folder,\nfollows renamed ids and fetches only the chosen folders. Fixes the tests\nthe removal of the five extensions left behind.\n\nPRs: #198, #307-#312, #316, #323, #324"),
 (153, 155, "Draw extension views as a designed tree of layout primitives",
  "Extension views become a tree of layout primitives with shared\ncomponents and tokens, a navigation stack, timed and pushed re-renders,\nstate kept by key, and List, Detail, Grid, canvas and Form rebuilt on\nit. Fixes the accumulated release matrix's first two waves.\n\nPRs: #121, #235-#243, #294, #326, #327"),
 (156, 156, "Version the extension API and ship 0.2 beside an adapter for 0.1",
  "Makes the WIT package's version the extension API number, fails lints\non incompatible changes without a new version, refuses extensions built\nfor versions Pane cannot run, ships API 0.2 with an adapter for 0.1 and\nrecords breaking changes in the release notes.\n\nPRs: #208, #317-#321, #325"),
 (157, 163, "Fix the release matrix after the designed views and API 0.2 landed",
  "Moves the samples to API 0.2, fixes the WIT guard, the keycap parse and\nJS form children, retires the typed-form templates, and fixes the\nsearch event's self-deadlock, the search field's keyboard and the\nSwitch Windows view. Folds in matrix fix-forwards 4-10.\n\nPRs: #328-#334"),
 (164, 174, "Restore designed views' focus and deliver their events and web answers",
  "Restores the window's focus ask, traces designed events through the\npark, pump and wasi:http boundary, fixes the search samples' service\npreference and the Rust SDK's text styles, and makes the tests wait for\ndesigned answers. Folds in matrix fix-forwards 11-21.\n\nPRs: #335-#345"),
 (175, 190, "Re-render designed views on every query and mint the Rust SDK's callback ids",
  "Treats the cleared listing as the empty state, drops a pending answer\nwith its text, mints callback ids in the Rust SDK's views and gives the\ntest twin the changes channel the real window has. Folds in matrix\nfix-forwards 22-37 and their diagnostics.\n\nPRs: #346-#361"),
 (191, 203, "Answer designed-view events before async work and settle the smoke faces",
  "The JS SDK answers designed-view events before async work and TSX nodes\ncarry keys, search samples clear stale rows, and the smokes read toasts\nwithin their three seconds and log their own key injections. Folds in\nmatrix fix-forwards 38-48 and parallel fixes 1-2.\n\nPRs: #362-#374"),
 (204, 209, "Let a newer search supersede in-flight work and keep guest calls served across a park",
  "Holds the park's wake registration, aborts a superseded search's web\nrequests after the turn, puts the guest's calls back when the instance\nsurvives a park, and makes the operations samples call greet at its\nversion.\n\nPRs: #375-#380"),
 (210, 220, "Migrate the tests to state-reading waits and fix the dependencies fixtures",
  "Moves the settle family to state-reading waits, declares the form\ncommands the dependencies fixtures launch, stops the services thread\nwhen the launcher drops, and delivers every event a render raised with\nsearch events in query order.\n\nPRs: #381-#391"),
 (221, 227, "Let only a search supersede a view's work, and fix designed toasts, templates and canvases",
  "Only a search event supersedes a view's work in flight; toast details\nkeep their focus, the form template scaffolds again, a canvas without a\nrole draws and a key-asking canvas gets navigation keys, and held keys\nsettle their typing.\n\nPRs: #392-#398"),
 (228, 237, "Fix the pane-core census: command screens, form keys, reloads and updates",
  "Keyboard focus returns only when a designed view closes, form fields\nare keyed, designed fields keep Backspace and submit only on Enter, and\nthe census tranches fix canvas limits, late opens, Switch Windows,\nreloads, the update trio, recovery tests and the handoff samples.\n\nPRs: #399-#408"),
 (238, 243, "Finish the designed-era migration and remove the spent diagnostics",
  "Joins designed keys as bindings spell them, keeps a designed List's\ntyped-ahead text, fixes the registrations roots, measures text-only\ncanvases, colours the operations answer by outcome and removes the\ndiagnostic scaffolds.\n\nPRs: #409-#414"),
 (244, 252, "Bring the GUI smokes back to green and pin the defaults for extension API 0.2",
  "Lets Pane's keyboard hook run while Pane is in front, tells assistive\ntechnology a hotkey's route, refuses root providers of another API\nshape, pins the defaults for API 0.2, pastes into elevated applications\nafter the target settles, and brings the GUI smokes back to green.\n\nPRs: #415-#420, #422, #423, #425"),
 (253, 258, "List the official extensions at first setup and steady the real-input tests",
  "First setup lists the official extensions with the defaults ticked, a\nfailed Pane update's row is selected again, the macOS icon worker is\nthrottled correctly, and the real-input hung-target and selection tests\nrun on CI again.\n\nPRs: #426-#431"),
 (259, 265, "Hide the macOS launcher's window controls and fix the remaining test races",
  "Keeps a filtered list row's icon, hides the macOS launcher's window\nbuttons and hides it on close, reads the selection by UI Automation,\ngives the Windows real-input tests native targets, and fixes the file\nindex, services and timers races.\n\nPRs: #436, #437, #438, #440, #441, #442, #443"),
 (266, 272, "Publish the SDKs, archive the old notes and rewrite the README and docs",
  "The SDKs are published on crates.io and npm and the defaults are pinned\nto releases built from them. Old evidence, handoff and research notes\nmove to an archive tag, the README is short and shows the launcher, and\nthe human-facing docs are rewritten.\n\nPRs: #444-#450"),
 (273, 276, "Release Pane through GitHub with app icons, version info and license notices",
  "Gives the packages an app icon, version info and license notices,\nkeeps Pane.app's Info.plist version current after an update, updates\nPane from GitHub Releases and drafts a prerelease when a version tag is\npushed.\n\nPRs: #452, #453, #454, #457, #459, #460, #461"),
 (277, 279, "Install Pane from a setup .exe, .dmg or AppImage, and keep the launcher off the taskbar",
  "Keeps the launcher off the taskbar and Alt+Tab, installs Pane on\nWindows with a setup .exe, and on macOS from a .dmg and Linux from an\nAppImage.\n\nPRs: #458, #462, #463, #464, #465, #473"),
 (280, 286, "Polish the launcher for Pane 0.1.0",
  "Arrow keys move a designed List's selection again, first setup shows\nthe official extensions' icons, the Windows-key tap works beside\nkey-masking tools and every hotkey press toggles the launcher, the\nanswer card is labelled and typeset, pushed views get a back button,\nand opening an app, file or link closes the launcher.\n\nPRs: #466-#472, #474, #476-#481"),
 (287, 287, "Pin the calculator that converts currencies",
  "Moves the calculator's pin to v0.6.4, which converts currencies.\n\nPRs: #475, #484"),
 (288, 288, "Give pushed views without a search bar a top bar with a back button",
  "A pushed view with no search bar now shows a top bar with a back\nbutton.\n\nPRs: #482, #485"),
 (289, 289, "Move Pane to version 0.1.1",
  "The first point release after v0.1.0, so the in-app updater can be\ntried going from 0.1.0 to 0.1.1.\n\nPRs: #483"),
]

# Validation
errs = []
expect = 1
for a, b, s, _ in G:
    if a != expect or b < a:
        errs.append(f"group {a}-{b} breaks contiguity (expected start {expect})")
    expect = b + 1
if expect != len(fp) + 1:
    errs.append(f"coverage ends at {expect - 1}, not {len(fp)}")
lasts = {fp[b - 1]["sha"][:8] for a, b, _, _ in G}
if V010 not in lasts:
    errs.append("v0.1.0 does not end a group")
for i in (287, 288, 289):
    if not any(a == b == i for a, b, _, _ in G):
        errs.append(f"commit {i} is not alone")
if errs:
    print("\n".join(errs))
    sys.exit(1)

plan, md = [], []
for n, (a, b, subj, body) in enumerate(G, 1):
    run = fp[a - 1:b]
    counts = collections.Counter()
    for c in run:
        counts.update(authors_of(c))
    ranked = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0] != HOANG))
    author = ranked[0][0]
    co = [x for x, _ in ranked[1:]]
    last = run[-1]
    plan.append({"first": run[0]["sha"], "last": last["sha"], "subject": subj,
                 "body": body, "author": author, "coauthors": co, "date": last["date"]})
    md.append(f"{n}. {last['date'][:10]}  {subj}  ({len(run)} commit{'s' if len(run) > 1 else ''})")

# Re-validate the JSON itself against the first-parent order
order = [c["sha"] for c in fp]
pos = 0
for g in plan:
    i, j = order.index(g["first"]), order.index(g["last"])
    assert i == pos and j >= i, g
    pos = j + 1
assert pos == len(order)
assert any(g["last"].startswith(V010) for g in plan)

with open(os.path.join(OUT, "history-groups.json"), "w", encoding="utf-8", newline="\n") as f:
    json.dump(plan, f, indent=2, ensure_ascii=False)
    f.write("\n")
with open(os.path.join(OUT, "history-groups.md"), "w", encoding="utf-8", newline="\n") as f:
    f.write("\n".join(md) + "\n")
print(f"OK: {len(plan)} groups cover {len(order)} first-parent commits; v0.1.0 ends group "
      f"{[k for k, g in enumerate(plan, 1) if g['last'].startswith(V010)][0]}")
print("authors:", collections.Counter(g["author"] for g in plan), "with coauthors:",
      sum(1 for g in plan if g["coauthors"]))
