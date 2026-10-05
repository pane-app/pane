// reference-capture.mjs - the visual workbench's reference side (#91).
//
// Opens the hash-pinned authored reference in an isolated headless Chrome,
// sets every board's `platform` to Windows (never the default macOS
// glyphs), saves each board's glass panel at its own logical size, and
// drives the root board through every reference-backed scenario of the
// native fixture's registry - the same steps, through Chrome's real input
// events - capturing the panel and its DOM state at each capture step.
//
//   node scripts/visual-workbench/reference-capture.mjs \
//     --reference docs/evidence/ui-prototype/reference/launcher.html \
//     --sha256 F7E81E03... --registry <registry.json> --out <dir>
//
// Isolation: the reference is served from a loopback-only HTTP server on a
// random port (file:// frames are cross-origin and block the boards'
// scripts), Chrome runs headless with a fresh temporary profile this
// script creates, and on exit the script ends only the Chrome process tree
// it started and deletes only the profile directory it created.
//
// Coordinates: device scale factor 1, so one CSS pixel is one image pixel;
// every rect the manifest records is relative to the board's glass panel,
// which is the native fixture's client.

import { spawn, execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, index, all) => {
    if (arg.startsWith('--')) pairs.push([arg.slice(2), all[index + 1]]);
    return pairs;
  }, []),
);
for (const required of ['reference', 'sha256', 'registry', 'out']) {
  if (!args[required]) {
    console.error(`reference-capture: --${required} is required`);
    process.exit(2);
  }
}
const CHROME = args.chrome ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const out = resolve(args.out);
mkdirSync(out, { recursive: true });
const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// The reference must be the pinned bytes: a changed file is not the
// authority this workbench compares against.
const html = readFileSync(args.reference);
const sha256 = createHash('sha256').update(html).digest('hex').toUpperCase();
if (sha256 !== args.sha256.toUpperCase()) {
  console.error(`reference-capture: ${args.reference} is ${sha256}, expected ${args.sha256}`);
  process.exit(3);
}
const registry = JSON.parse(readFileSync(args.registry, 'utf8'));

// Board slugs by the board document's title, as the research catalogued
// them; Store and the snap HUD are saved as source-only references.
const BOARDS = [
  ['root', /launcher$/i],
  ['actions', /action panel/i],
  ['calculator', /calculator/i],
  ['empty', /no results/i],
  ['clipboard', /clipboard/i],
  ['window-manager', /window manager/i],
  ['store', /plugin store/i],
  ['settings', /settings/i],
  ['design-language', /design language/i],
];
const SOURCE_ONLY = new Set(['store', 'window-manager']);

const server = createServer((request, response) => {
  response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
  response.end(html);
});
await new Promise((ready) => server.listen(0, '127.0.0.1', ready));
const pageUrl = `http://127.0.0.1:${server.address().port}/launcher.html`;

const profile = mkdtempSync(join(tmpdir(), 'pane-visual-workbench-chrome-'));
const debugPort = 9400 + Math.floor(Math.random() * 400);
// One tall viewport holds every board, so viewport and document
// coordinates agree and nothing scrolls between measuring and capturing.
const chrome = spawn(
  CHROME,
  [
    `--remote-debugging-port=${debugPort}`,
    `--user-data-dir=${profile}`,
    '--headless=new',
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions',
    '--force-device-scale-factor=1',
    '--hide-scrollbars',
    '--window-size=1600,9600',
    'about:blank',
  ],
  { stdio: 'ignore' },
);

const manifest = {
  kind: 'pane-visual-reference-manifest',
  reference: resolve(args.reference),
  sha256,
  chrome: null,
  chromeProfile: profile,
  deviceScaleFactor: 1,
  platform: 'Windows',
  startedUtc: new Date().toISOString(),
  boards: [],
  scenarios: [],
  fonts: null,
  cleanup: null,
};

let socket;
let nextId = 0;
const waiting = new Map();
const call = (method, params = {}) =>
  new Promise((done, fail) => {
    const id = ++nextId;
    waiting.set(id, (message) =>
      message.error ? fail(new Error(`${method}: ${message.error.message}`)) : done(message.result),
    );
    socket.send(JSON.stringify({ id, method, params }));
  });
const evaluate = async (expression) => {
  const result = await call('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) {
    throw new Error(`page script failed: ${result.exceptionDetails.exception?.description ?? result.exceptionDetails.text}`);
  }
  return result.result.value;
};

// Page-side helpers, installed once per load: the board frames by slug,
// and the root board's DOM state relative to its glass panel.
const HELPERS = `
window.__wb = {
  frames() {
    return Array.from(document.querySelectorAll('iframe')).map((frame) => {
      const doc = frame.contentDocument;
      const r = frame.getBoundingClientRect();
      const glass = doc && doc.querySelector('section.glass');
      const g = glass && glass.getBoundingClientRect();
      return { title: doc ? doc.title : null, frame: [r.x, r.y, r.width, r.height],
               glass: g ? [g.x, g.y, g.width, g.height] : null,
               hasProps: !!(frame.contentWindow && frame.contentWindow.__dcSetProps) };
    });
  },
  frame(index) { return document.querySelectorAll('iframe')[index]; },
  // Board frames sit at fractional page offsets (headings above them);
  // shift each by its fraction so every board renders on whole pixels and
  // a capture never resamples it.
  snap() {
    for (const frame of document.querySelectorAll('iframe')) {
      frame.style.position = 'relative';
      frame.style.left = '0px';
      frame.style.top = '0px';
      const r = frame.getBoundingClientRect();
      frame.style.left = -(r.x - Math.floor(r.x)) + 'px';
      frame.style.top = -(r.y - Math.floor(r.y)) + 'px';
    }
    return Array.from(document.querySelectorAll('iframe')).every((f) => {
      const r = f.getBoundingClientRect();
      return Number.isInteger(r.x) && Number.isInteger(r.y);
    });
  },
  setWindows(index) {
    const w = this.frame(index).contentWindow;
    w.__dcSetProps(w.__dcRootName(), { platform: 'Windows', wallpaper: 'Graphite', accent: '#C9EE6A' });
  },
  rel(index, el) {
    const doc = this.frame(index).contentDocument;
    const g = doc.querySelector('section.glass').getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return { x: r.x - g.x, y: r.y - g.y, width: r.width, height: r.height };
  },
  outer(index, el) {
    const f = this.frame(index).getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return { x: f.x + r.x + r.width / 2, y: f.y + r.y + r.height / 2 };
  },
  rowFor(index, title) {
    const doc = this.frame(index).contentDocument;
    return Array.from(doc.querySelectorAll('button.row')).find((row) => row.querySelector('.row-t')?.textContent === title);
  },
  // The results' container: the root board's scrolling \`.list\`, or the
  // column after the search header on a static board that has none (the
  // Actions board).
  listEl(index) {
    const doc = this.frame(index).contentDocument;
    return doc.querySelector('.list') || doc.querySelector('input.q').parentElement.nextElementSibling;
  },
  rows(index) {
    const doc = this.frame(index).contentDocument;
    const list = this.listEl(index).getBoundingClientRect();
    return Array.from(doc.querySelectorAll('button.row')).map((row) => {
      const rect = this.rel(index, row);
      const r = row.getBoundingClientRect();
      const tile = row.querySelector('.tile');
      // A calculator board's history row titles its expression in a
      // .mono span, with no .row-t (#96).
      const title = row.querySelector('.row-t') || row.querySelector('.mono');
      const sub = row.querySelector('.row-s');
      return {
        title: title.textContent,
        subtitle: sub ? sub.textContent : null,
        selected: row.classList.contains('sel'),
        hovered: row.matches(':hover'),
        visible: r.top >= list.top && r.bottom <= list.bottom,
        rect,
        tile: tile ? this.rel(index, tile) : null,
        tileApp: tile ? tile.classList.contains('app') : null,
        titleRect: this.rel(index, title),
        background: getComputedStyle(row).backgroundColor,
        keys: Array.from(row.querySelectorAll('.keys .kbd')).map((k) => ({ label: k.textContent, rect: this.rel(index, k) })),
        keyGroup: this.group(index, row.querySelector('.keys')),
        kind: (() => { const k = row.querySelector('.row-kind'); return k ? { text: k.textContent, rect: this.rel(index, k) } : null; })(),
        alias: (() => { const a = row.querySelector('.alias'); return a ? { text: a.textContent, rect: this.rel(index, a) } : null; })(),
      };
    });
  },
  labels(index) {
    const container = this.listEl(index);
    const list = container.getBoundingClientRect();
    return Array.from(container.querySelectorAll('.label')).map((label) => {
      const spans = label.querySelectorAll(':scope > span');
      const r = label.getBoundingClientRect();
      return {
        text: spans[0] ? spans[0].textContent : label.textContent,
        note: spans[1] ? spans[1].textContent.trim() : '',
        rect: this.rel(index, label),
        visible: r.top >= list.top && r.bottom <= list.bottom,
      };
    });
  },
  group(index, el) {
    if (!el) return null;
    const caps = el.classList.contains('kbd') ? [el] : Array.from(el.querySelectorAll('.kbd'));
    const style = getComputedStyle(caps[0]);
    return {
      rect: this.rel(index, el),
      caps: caps.map((k) => ({ label: k.textContent, rect: this.rel(index, k) })),
      font: { family: style.fontFamily, size: style.fontSize, weight: style.fontWeight },
      background: style.backgroundColor,
    };
  },
  keycapGroups(index) {
    const doc = this.frame(index).contentDocument;
    const buttons = doc.querySelectorAll('.fbtn');
    const rowKeys = (title) => { const row = this.rowFor(index, title); return row && row.querySelector('.keys'); };
    return {
      'footer-actions': this.group(index, buttons[1] && buttons[1].querySelector('.keys')),
      'left-half': this.group(index, rowKeys('Left Half')),
      'clipboard-history': this.group(index, rowKeys('Clipboard History')),
      'footer-primary': this.group(index, buttons[0] && buttons[0].querySelector('.kbd')),
      // The first pinned slot's compact corner hint (Ctrl 1).
      'pinned-1': this.group(index, doc.querySelector('.slot .slot-k')),
    };
  },
  // The footer's parts: the mark (its svg), the hint's parts left to
  // right (a cap or a sequence of caps, or text), the rule between the
  // buttons and the Actions button.
  footerParts(index) {
    const doc = this.frame(index).contentDocument;
    const buttons = Array.from(doc.querySelectorAll('.fbtn'));
    let strip = buttons[0];
    while (strip && strip.parentElement && !strip.parentElement.matches('section.glass')) strip = strip.parentElement;
    if (!strip) return null;
    const left = strip.children[0];
    const mark = left && left.querySelector(':scope > svg');
    const hintEl = left && Array.from(left.children).find((child) => child.tagName !== 'svg' && child.tagName !== 'SVG');
    let hint = [];
    if (hintEl && hintEl.children.length) {
      hint = Array.from(hintEl.children).map((part) => ({
        text: part.textContent.trim(),
        keys: part.classList.contains('kbd') || part.classList.contains('keys'),
        rect: this.rel(index, part),
      }));
    } else if (hintEl) {
      hint = [{ text: hintEl.textContent.trim(), keys: false, rect: this.rel(index, hintEl) }];
    }
    const right = strip.children[1];
    const divider = right && Array.from(right.children).find((child) => !child.classList.contains('fbtn'));
    const actions = buttons.find((button) => button.textContent.trim().startsWith('Actions'));
    return {
      mark: mark ? this.rel(index, mark) : null,
      hint,
      divider: divider ? this.rel(index, divider) : null,
      actionsButton: actions ? {
        rect: this.rel(index, actions),
        pressed: actions.getAttribute('aria-pressed') === 'true',
        background: getComputedStyle(actions).backgroundColor,
        keyGroup: this.group(index, actions.querySelector('.keys')),
      } : null,
    };
  },
  // The open Actions panel (\`.pop\`): its header, entries, group labels,
  // rules, empty note and search row, and the dimmer over the results.
  panel(index) {
    const doc = this.frame(index).contentDocument;
    const pop = doc.querySelector('.pop');
    if (!pop) return null;
    const parts = Array.from(pop.children);
    const search = parts[parts.length - 1];
    const body = parts[parts.length - 2];
    const header = parts.length >= 3 ? parts[0] : null;
    const label = (row) => Array.from(row.children).find((child) => child.tagName === 'SPAN' && !child.classList.contains('kbd') && !child.classList.contains('keys'));
    const rows = Array.from(body.querySelectorAll('.arow')).map((row) => ({
      label: label(row).textContent.trim(),
      rect: this.rel(index, row),
      selected: row.classList.contains('sel'),
      glyph: this.rel(index, row.querySelector('svg')),
      labelRect: this.rel(index, label(row)),
      keyGroup: this.group(index, row.querySelector('.keys') || row.querySelector(':scope > .kbd')),
      background: getComputedStyle(row).backgroundColor,
    }));
    const empty = Array.from(body.querySelectorAll('div')).find((div) => div.textContent.trim() === 'No actions match');
    const input = search.querySelector('input');
    const dimmer = Array.from(doc.querySelectorAll('section.glass > div[aria-hidden="true"]'))[0];
    const tile = header && header.querySelector('.tile');
    const title = header && Array.from(header.children).find((child) => !child.classList.contains('tile'));
    return {
      rect: this.rel(index, pop),
      header: header ? {
        rect: this.rel(index, header),
        title: title ? title.textContent.trim() : '',
        titleRect: title ? this.rel(index, title) : null,
        tile: tile ? this.rel(index, tile) : null,
        tileApp: tile ? tile.classList.contains('app') : null,
      } : null,
      rows,
      groups: Array.from(body.querySelectorAll('.alabel')).map((group) => ({ text: group.textContent.trim(), rect: this.rel(index, group) })),
      rules: Array.from(body.querySelectorAll('.sep')).map((rule) => this.rel(index, rule)),
      empty: empty ? { text: empty.textContent.trim(), rect: this.rel(index, empty) } : null,
      search: this.rel(index, search),
      field: input ? { rect: this.rel(index, input), value: input.value, placeholder: input.placeholder, focused: doc.activeElement === input } : null,
      dimmer: dimmer ? { rect: this.rel(index, dimmer), background: getComputedStyle(dimmer).backgroundColor } : null,
    };
  },
  // The pinned home (#101): the "Pinned" label, the slots' grid, and each
  // slot's box, tile, title and compact key hint; null while hidden.
  pinned(index) {
    const doc = this.frame(index).contentDocument;
    const slots = Array.from(doc.querySelectorAll('.slot'));
    if (!slots.length) return null;
    const label = Array.from(doc.querySelectorAll('.label')).find((l) => (l.querySelector(':scope > span') || l).textContent.trim() === 'Pinned');
    return {
      label: label ? this.rel(index, label) : null,
      labelKeys: label ? this.group(index, label.querySelectorAll(':scope > span')[1]) : null,
      strip: this.rel(index, slots[0].parentElement),
      slots: slots.map((slot, i) => {
        const tile = slot.querySelector('.tile');
        const title = slot.querySelector('.slot-t');
        return {
          number: i + 1,
          title: title ? title.textContent : null,
          rect: this.rel(index, slot),
          tile: tile ? this.rel(index, tile) : null,
          tileApp: tile ? tile.classList.contains('app') : null,
          titleRect: title ? this.rel(index, title) : null,
          hovered: slot.matches(':hover'),
          background: getComputedStyle(slot).backgroundColor,
          keyGroup: this.group(index, slot.querySelector('.slot-k')),
        };
      }),
    };
  },
  state(index) {
    const doc = this.frame(index).contentDocument;
    const q = doc.querySelector('input.q');
    const buttons = doc.querySelectorAll('.fbtn');
    return {
      glass: this.rel(index, doc.querySelector('section.glass')),
      query: q.value,
      focused: doc.activeElement === q,
      searchHeader: this.rel(index, q.parentElement),
      list: this.rel(index, this.listEl(index)),
      listScrollTop: this.listEl(index).scrollTop,
      footerPrimary: buttons[0] ? { label: buttons[0].textContent.trim(), rect: this.rel(index, buttons[0]) } : null,
      // Every footer button, left to right (the primary action, then
      // Actions): the last one's right edge is the footer's right padding.
      footerButtons: Array.from(buttons).map((b) => ({ label: b.textContent.trim(), rect: this.rel(index, b) })),
      // The footer strip: the glass panel's child that holds the buttons.
      footer: (() => {
        let el = buttons[0];
        while (el && el.parentElement && !el.parentElement.matches('section.glass')) el = el.parentElement;
        return el ? this.rel(index, el) : null;
      })(),
      rows: this.rows(index),
      labels: this.labels(index),
      keycaps: this.keycapGroups(index),
      footerParts: this.footerParts(index),
      panel: this.panel(index),
      pinned: this.pinned(index),
    };
  },
  // The Settings board (#97): its titlebar and label, the sidebar with its
  // search well and its sections (each one's wash, label, glyph and count),
  // and the page's heading, subtitle and preview column's caption.
  settingsState(index) {
    const doc = this.frame(index).contentDocument;
    const glass = doc.querySelector('section.glass');
    const bar = glass.children[0];
    const title = bar.querySelector('span');
    const body = glass.children[1];
    const nav = body.querySelector('nav');
    const label = nav.querySelector('label');
    const input = label.querySelector('input');
    const content = body.children[1];
    const heading = content.querySelector('h1');
    const subtitle = heading.nextElementSibling;
    const columns = Array.from(content.children);
    const caption = columns[1] ? columns[1].children[0] : null;
    return {
      glass: this.rel(index, glass),
      titlebar: {
        rect: this.rel(index, bar),
        title: { text: title.textContent, rect: this.rel(index, title) },
        close: this.rel(index, bar.querySelector('button')),
      },
      sidebar: { rect: this.rel(index, nav), background: getComputedStyle(nav).backgroundColor },
      search: {
        rect: this.rel(index, label),
        background: getComputedStyle(label).backgroundColor,
        icon: this.rel(index, label.querySelector('svg')),
        input: this.rel(index, input),
        placeholder: input.placeholder,
        focused: doc.activeElement === input,
      },
      items: Array.from(nav.querySelectorAll('button.nav')).map((item) => {
        const spans = item.querySelectorAll(':scope > span');
        return {
          label: spans[0].textContent,
          rect: this.rel(index, item),
          selected: item.classList.contains('on'),
          hovered: item.matches(':hover'),
          background: getComputedStyle(item).backgroundColor,
          color: getComputedStyle(item).color,
          glyph: this.rel(index, item.querySelector('svg')),
          labelRect: this.rel(index, spans[0]),
          count: spans[1] ? { text: spans[1].textContent, rect: this.rel(index, spans[1]) } : null,
        };
      }),
      heading: { text: heading.textContent, rect: this.rel(index, heading) },
      subtitle: { text: subtitle.textContent, rect: this.rel(index, subtitle) },
      aside: caption ? { text: caption.textContent, rect: this.rel(index, caption) } : null,
      columns: columns.map((column) => this.rel(index, column)),
    };
  },
  // The clipboard board's logic scrolls a clip the keys select into view,
  // 8px past the list's edge (componentDidUpdate, through the global
  // document), which this board runtime never runs against the board's
  // frame: its list stays where it was. The capture scrolls it as that
  // logic would, with the same arithmetic; a clip in view is left alone.
  clipboardScroll(index) {
    const doc = this.frame(index).contentDocument;
    const el = doc.querySelector('.row.sel');
    const list = el && el.closest('.list');
    if (!el || !list) return null;
    const r = el.getBoundingClientRect();
    const lr = list.getBoundingClientRect();
    const scale = lr.height / (list.offsetHeight || 1);
    if (r.bottom > lr.bottom) list.scrollTop += (r.bottom - lr.bottom) / scale + 8;
    else if (r.top < lr.top) list.scrollTop -= (lr.top - r.top) / scale + 8;
    return list.scrollTop;
  },
  // The clipboard board (#102): its header, tab strip, list (rows, day
  // labels, the note in place of rows), preview card and footer, relative
  // to its glass panel. The preview's branch is told by the card's first
  // child: the code lines (.mono), the hatch (image), the 44px tile
  // (link), a color's values (.mono spans), else plain text.
  clipboardState(index) {
    const doc = this.frame(index).contentDocument;
    const glass = doc.querySelector('section.glass');
    const [header, strip, body, footer] = Array.from(glass.children);
    const q = doc.querySelector('input.q');
    const list = body.children[0];
    const pane = body.children[1];
    const lr = list.getBoundingClientRect();
    const rows = Array.from(list.querySelectorAll('button.row')).map((row) => {
      const r = row.getBoundingClientRect();
      const title = row.children[1];
      const time = row.children[2];
      return {
        title: title ? title.textContent : '',
        time: time ? time.textContent : '',
        rect: this.rel(index, row),
        selected: row.classList.contains('sel'),
        hovered: row.matches(':hover'),
        visible: r.top >= lr.top && r.bottom <= lr.bottom,
        background: getComputedStyle(row).backgroundColor,
        mark: this.rel(index, row.children[0]),
        titleRect: title ? this.rel(index, title) : null,
        timeRect: time ? this.rel(index, time) : null,
      };
    });
    const labels = Array.from(list.querySelectorAll('.label')).map((label) => {
      const r = label.getBoundingClientRect();
      return { text: label.children[0].textContent, rect: this.rel(index, label), visible: r.top >= lr.top && r.bottom <= lr.bottom };
    });
    const emptyEl = Array.from(list.children).find((child) => child.tagName === 'DIV' && !child.classList.contains('label'));
    const card = pane.children[0] || null;
    let kind = null;
    if (card) {
      const first = card.children[0];
      if (!first) kind = null;
      else if (first.classList.contains('mono')) kind = 'code';
      else if (first.classList.contains('hatch')) kind = 'image';
      else if (first.querySelector('.tile')) kind = 'link';
      else if (first.querySelector('.mono')) kind = 'color';
      else kind = 'text';
    }
    const lead = footer.children[0];
    const copiedText = lead && lead.children[lead.children.length - 1];
    const right = footer.children[1];
    const divider = right && Array.from(right.children).find((child) => !child.classList.contains('fbtn'));
    const capture = header.querySelector('.fbtn');
    const caption = strip.children[strip.children.length - 1];
    return {
      glass: this.rel(index, glass),
      query: q.value,
      focused: doc.activeElement === q,
      header: this.rel(index, header),
      strip: this.rel(index, strip),
      list: this.rel(index, list),
      listScrollTop: list.scrollTop,
      pane: this.rel(index, pane),
      card: card ? { rect: this.rel(index, card), background: getComputedStyle(card).backgroundColor, kind } : null,
      rows,
      labels,
      empty: emptyEl ? { text: emptyEl.textContent.trim(), rect: this.rel(index, emptyEl) } : null,
      back: { rect: this.rel(index, header.children[0]), background: getComputedStyle(header.children[0]).backgroundColor },
      chip: { rect: this.rel(index, header.children[1]), background: getComputedStyle(header.children[1]).backgroundColor },
      capture: capture ? { label: capture.textContent.trim(), rect: this.rel(index, capture) } : null,
      tabs: Array.from(strip.querySelectorAll('button.tab')).map((tab) => ({
        label: tab.textContent.trim(),
        on: tab.classList.contains('on'),
        rect: this.rel(index, tab),
        background: getComputedStyle(tab).backgroundColor,
      })),
      caption: caption ? { text: caption.textContent.trim(), rect: this.rel(index, caption) } : null,
      footer: this.rel(index, footer),
      copied: copiedText ? { text: copiedText.textContent.trim(), rect: this.rel(index, copiedText) } : null,
      buttons: Array.from(footer.querySelectorAll('.fbtn')).map((button) => ({
        label: button.children[0] ? button.children[0].textContent.trim() : button.textContent.trim(),
        rect: this.rel(index, button),
      })),
      divider: divider ? this.rel(index, divider) : null,
    };
  },
  fonts() {
    const doc = this.frame(0).contentDocument;
    return Array.from(doc.fonts).map((f) => ({ family: f.family, weight: f.weight, style: f.style, status: f.status }));
  },
  // A board's own font faces and whether each loaded (#103: a face that
  // failed or never loaded would draw the board in a fallback). Every board
  // is its own document with its own FontFaceSet.
  fontsOf(index) {
    const doc = this.frame(index).contentDocument;
    return Array.from(doc.fonts).map((f) => ({
      family: f.family.replace(/^["']|["']$/g, ''), weight: f.weight, style: f.style, status: f.status,
    }));
  },
  fontsReady(index) {
    const frames = index === undefined ? Array.from(document.querySelectorAll('iframe')) : [this.frame(index)];
    return Promise.all(frames.map((f) => (f.contentDocument ? f.contentDocument.fonts.ready : null))).then(() => true);
  },
};
true`;

// The result boards' own parts (#96), added to every capture's DOM state:
// the empty board's no-results notice heading the list, the calculator
// board's answer card, and the empty board's extension suggestions after
// the rows - each null or empty on a board without it.
const RESULT_HELPERS = `
Object.assign(window.__wb, {
  notice(index) {
    const list = this.listEl(index);
    const el = Array.from(list.children).find((child) => child.tagName === 'DIV' && !child.className && getComputedStyle(child).height === '84px');
    if (!el || el.children.length < 2) return null;
    const [disc, text] = el.children;
    const [title, description] = text.children;
    return {
      rect: this.rel(index, el),
      disc: this.rel(index, disc),
      discBackground: getComputedStyle(disc).backgroundColor,
      title: { text: title.textContent, rect: this.rel(index, title) },
      description: { text: description.textContent, rect: this.rel(index, description) },
    };
  },
  answer(index) {
    const list = this.listEl(index);
    const card = Array.from(list.children).find((child) => child.tagName === 'BUTTON' && !child.classList.contains('row') && child.children[0] && getComputedStyle(child.children[0]).display === 'grid');
    if (!card) return null;
    const [grid, also] = card.children;
    const [left, arrow, right] = grid.children;
    const side = (column) => {
      const [value, caption] = column.children;
      return {
        value: { text: value.textContent, rect: this.rel(index, value) },
        caption: caption ? { text: caption.textContent, rect: this.rel(index, caption) } : null,
      };
    };
    const style = getComputedStyle(card);
    return {
      rect: this.rel(index, card),
      selected: style.boxShadow.includes('201, 238, 106'),
      background: style.backgroundColor,
      source: side(left),
      answer: side(right),
      arrow: this.rel(index, arrow),
      arrowBackground: getComputedStyle(arrow).backgroundColor,
      also: also ? { text: also.children[0].textContent, rect: this.rel(index, also.children[0]) } : null,
      chips: also ? Array.from(also.querySelectorAll('.chip')).map((chip) => ({ text: chip.textContent, rect: this.rel(index, chip) })) : [],
      rule: also ? this.rel(index, also) : null,
    };
  },
  suggestions(index) {
    const list = this.listEl(index);
    return Array.from(list.querySelectorAll(':scope > .prow')).map((row) => {
      const pill = row.querySelector('.pill');
      return {
        title: row.querySelector(':scope > div').children[0].textContent,
        rect: this.rel(index, row),
        tile: this.rel(index, row.querySelector('.tile')),
        pill: this.rel(index, pill),
        pillBackground: getComputedStyle(pill).backgroundColor,
      };
    });
  },
});
const rootState = window.__wb.state;
window.__wb.state = function (index) {
  return Object.assign(rootState.call(this, index), {
    notice: this.notice(index),
    answer: this.answer(index),
    suggestions: this.suggestions(index),
  });
};
true`;

// The Settings board's Appearance page (#98), added to the Settings
// board's capture state: its controls column - the field labels, the
// segmented choices and their segments, the descriptions, the swatches,
// the sliders and the toggles - and its preview - the caption, the stage,
// the miniature's search line, slots, rows and footer - and the link under
// it, each relative to the glass panel. A part's opacity is the product of
// its ancestors' up to the column (the board dims Solid's sliders to 40%).
const APPEARANCE_HELPERS = `
Object.assign(window.__wb, {
  appearanceState(index) {
    const doc = this.frame(index).contentDocument;
    const glass = doc.querySelector('section.glass');
    const content = glass.children[1].children[1];
    const [column, aside] = content.children;
    const rel = (el) => this.rel(index, el);
    const inner = (el) => el.querySelector('.sc-interp') || el;
    const text = (el) => (el ? { text: el.textContent.trim(), rect: rel(inner(el)) } : null);
    const opacity = (el) => {
      let value = 1;
      for (let at = el; at && at !== column; at = at.parentElement) value *= Number(getComputedStyle(at).opacity);
      return value;
    };
    const labels = Array.from(column.querySelectorAll('.flabel'))
      .filter((label) => label.parentElement.parentElement === column)
      .map((label) => ({ text: label.textContent.trim(), rect: rel(label), opacity: opacity(label) }));
    const tracks = Array.from(column.querySelectorAll('.segwrap')).map((track) => {
      const name = doc.getElementById(track.getAttribute('aria-labelledby'));
      return {
        field: name ? name.textContent.trim().toLowerCase() : '',
        rect: rel(track),
        opacity: opacity(track),
        background: getComputedStyle(track).backgroundColor,
        segments: Array.from(track.querySelectorAll('button')).map((segment) => ({
          label: segment.textContent.trim(),
          rect: rel(segment),
          labelBox: rel(inner(segment)),
          chosen: segment.classList.contains('on'),
          hovered: segment.matches(':hover'),
          background: getComputedStyle(segment).backgroundColor,
          color: getComputedStyle(segment).color,
        })),
      };
    });
    const descriptions = Array.from(column.querySelectorAll('.fdesc')).map((description) => ({
      text: description.textContent.trim(), rect: rel(description), opacity: opacity(description),
    }));
    const swatches = Array.from(column.querySelectorAll('button.sw'))
      .filter((swatch) => swatch.getAttribute('aria-label') !== 'Custom color')
      .map((swatch) => ({ name: swatch.getAttribute('aria-label'), rect: rel(swatch),
                          chosen: swatch.getAttribute('aria-pressed') === 'true' }));
    const sliders = Array.from(column.querySelectorAll('input.range')).map((range) => {
      const header = range.parentElement.children[0];
      const value = header.querySelector('.mono');
      return {
        label: text(header.querySelector('label')),
        // The whole value ("44 px"), not its interpolated number alone.
        value: { text: value.textContent.trim(), rect: rel(value) },
        rect: rel(range),
        opacity: opacity(range),
        disabled: range.disabled,
      };
    });
    const toggles = Array.from(column.querySelectorAll('button'))
      .filter((button) => !button.classList.contains('seg') && !button.classList.contains('sw'))
      .map((toggle) => ({
        label: text(toggle.parentElement.querySelector('.flabel')),
        rect: rel(toggle),
        knob: rel(toggle.children[0]),
        on: toggle.getAttribute('aria-pressed') === 'true',
      }));
    let preview = null;
    if (aside && aside.children.length >= 2) {
      const stage = aside.children[1];
      const mini = stage.children[stage.children.length - 1];
      const [search, body, footer] = mini.children;
      const strip = Array.from(body.children).find((child) => !child.classList.contains('mrow'));
      preview = {
        caption: rel(aside.children[0]),
        stage: rel(stage),
        panel: rel(mini),
        search: rel(search),
        query: text(search.children[1]),
        caret: rel(search.children[2]),
        pins: strip ? Array.from(strip.children).map((slot) => rel(slot)) : [],
        rows: Array.from(body.querySelectorAll('.mrow')).map((row) => ({
          title: text(row.children[1]),
          kind: text(row.children[2]),
          rect: rel(row),
          tile: rel(row.children[0]),
          selected: getComputedStyle(row).backgroundColor !== 'rgba(0, 0, 0, 0)',
        })),
        footer: rel(footer),
        tip: text(footer.children[0]),
        keys: rel(footer.querySelector('.kbd')),
      };
    }
    const link = aside && aside.querySelector('a');
    return { column: rel(column), labels, tracks, descriptions, swatches, sliders, toggles, preview,
             link: link ? { text: link.textContent.trim(), rect: rel(link) } : null };
  },
});
const settingsShell = window.__wb.settingsState;
window.__wb.settingsState = function (index) {
  return Object.assign(settingsShell.call(this, index), { appearance: this.appearanceState(index) });
};
true`;

async function load() {
  await call('Page.navigate', { url: pageUrl });
  for (let attempt = 0; attempt < 120; attempt++) {
    await sleep(250);
    const ready = await evaluate(
      `Array.from(document.querySelectorAll('iframe')).length >= 9 && Array.from(document.querySelectorAll('iframe')).every((f) => f.contentWindow && f.contentWindow.__dcSetProps && f.contentDocument.readyState === 'complete')`,
    ).catch(() => false);
    if (ready) break;
  }
  await evaluate(HELPERS);
  await evaluate(RESULT_HELPERS);
  await evaluate(APPEARANCE_HELPERS);
  if (!(await evaluate('__wb.snap()'))) throw new Error('the board frames could not be snapped to whole pixels');
  const frames = await evaluate('__wb.frames()');
  for (let index = 0; index < frames.length; index++) {
    if (frames[index].hasProps) await evaluate(`__wb.setWindows(${index})`);
  }
  await evaluate('document.fonts.ready.then(() => true)');
  await evaluate('__wb.fontsReady()');
  await sleep(700);
  return evaluate('__wb.frames()');
}

async function screenshot(file, frame, glass) {
  const clip = { x: frame[0] + glass[0], y: frame[1] + glass[1], width: glass[2], height: glass[3], scale: 1 };
  const shot = await call('Page.captureScreenshot', { format: 'png', clip, captureBeyondViewport: false });
  const bytes = Buffer.from(shot.data, 'base64');
  writeFileSync(file, bytes);
  return { file, width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20), clip };
}

const KEYS = {
  down: { key: 'ArrowDown', code: 'ArrowDown', windowsVirtualKeyCode: 40 },
  up: { key: 'ArrowUp', code: 'ArrowUp', windowsVirtualKeyCode: 38 },
  escape: { key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 },
};

// Where this script last put Chrome's pointer, in page coordinates. Headless
// Chrome has no OS cursor: this is the only pointer the boards ever see.
let pointerAt = null;

async function pointerTo(x, y) {
  pointerAt = [x, y];
  await call('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none' });
}

// Parks the pointer on the page outside every board before a scenario
// (#103), so where an earlier scenario left it can never hover a board.
async function parkPointer(frames) {
  const inside = ([x, y]) => frames.some(({ frame: [fx, fy, fw, fh] }) => x >= fx && x < fx + fw && y >= fy && y < fy + fh);
  const spot = [[2, 2], [1597, 2], [2, 9597], [1597, 9597]].find((point) => !inside(point));
  if (!spot) throw new Error('no place on the page lies outside every board to park the pointer');
  await pointerTo(spot[0], spot[1]);
}

// Where the pointer is relative to a board's glass panel, or null when it
// lies off the panel.
function pointerOnBoard(frame, glass) {
  if (!pointerAt) return null;
  const x = pointerAt[0] - frame[0] - glass[0];
  const y = pointerAt[1] - frame[1] - glass[1];
  return x >= 0 && y >= 0 && x < glass[2] && y < glass[3] ? [x, y] : null;
}

// The elements a click step names, found on the board by what they show.
const clipRow = (title) => (index) =>
  `Array.from(__wb.frame(${index}).contentDocument.querySelectorAll('.list button.row')).find((b) => b.children[1].textContent === ${JSON.stringify(title)})`;
const clipTab = (label) => (index) =>
  `Array.from(__wb.frame(${index}).contentDocument.querySelectorAll('button.tab')).find((b) => b.textContent.trim() === ${JSON.stringify(label)})`;
// The Appearance board's segments (#98), `segment-<label>` in lower case,
// as the fixture names them.
const segment = (label) => (index) =>
  `Array.from(__wb.frame(${index}).contentDocument.querySelectorAll('.segwrap button')).find((b) => b.textContent.trim() === ${JSON.stringify(label)})`;
const SEGMENTS = Object.fromEntries(['Glass', 'Frost', 'Solid', 'Compact', 'Default', 'Roomy']
  .map((label) => [`segment-${label.toLowerCase()}`, segment(label)]));
const CLICK_TARGETS = {
  'actions-button': (index) => `Array.from(__wb.frame(${index}).contentDocument.querySelectorAll('.fbtn')).find((b) => b.textContent.trim().startsWith('Actions'))`,
  // The clipboard board's clips and tabs (#102), by the fixture's ids.
  'clip-standup': clipRow('Standup moved to 10:30 tomorrow'),
  'clip-lime': clipRow('#C9EE6A'),
  'clip-link': clipRow('example.com/plugins/manifest'),
  'clip-shot': clipRow('Screenshot 2880 × 1800'),
  'clip-tab-text': clipTab('Text'),
  ...SEGMENTS,
};

// A board's capture state, by board: the root family's state by default,
// the Settings board's own (#97) and the clipboard board's own (#102).
const STATES = { settings: 'settingsState', clipboard: 'clipboardState' };

// The elements a pointer step names on a static board that takes them, by
// board: the Settings board's sidebar sections, in their order (#97). Its
// sections answer to the pointer through CSS alone (`.nav:hover`), so the
// board's authored state is otherwise untouched.
const POINTER_TARGETS = {
  settings: (index, row) => `__wb.frame(${index}).contentDocument.querySelectorAll('nav button.nav')[${row}]`,
};

// The boards a scenario drives with its steps, as the native fixture is
// driven; any other board authors its one state, captured as it is.
const INTERACTIVE = new Set(['root', 'clipboard']);

// The elements a point step names: the pinned slots, `slot-<n>` from 1,
// and the Appearance board's segments (#98).
const POINT_TARGETS = {
  ...Object.fromEntries([1, 2, 3, 4, 5].map((n) => [
    `slot-${n}`,
    (index) => `__wb.frame(${index}).contentDocument.querySelectorAll('.slot')[${n - 1}]`,
  ])),
  ...SEGMENTS,
};

// The steps a static board takes as authored: the Settings board's
// sections answer the pointer (`.nav:hover`), and its Appearance page's
// segments the pointer (`.seg:hover`) and a click, which its logic takes
// as the native fixture does (#98). Every other step only brings the
// native fixture to the board's state.
const AUTHORED_STEPS = { settings: new Set(['pointer', 'point', 'click']) };

async function runScenario(scenario, rootIndex, frames) {
  const dir = join(out, scenario.name);
  mkdirSync(dir, { recursive: true });
  // A static board (any but the interactive root) authors its one state:
  // it is captured as authored, and the scenario's steps — which bring the
  // native fixture to that state — are not taken here, except the ones the
  // board answers itself (AUTHORED_STEPS: the Settings board's pointer,
  // point and click steps).
  const boardIndex = scenario.board
    ? frames.findIndex((frame) => (BOARDS.find(([slug]) => slug === scenario.board) ?? [null, /^$/])[1].test(frame.title ?? ''))
    : rootIndex;
  if (boardIndex < 0) throw new Error(`the reference has no ${scenario.board} board`);
  const authored = boardIndex !== rootIndex && !INTERACTIVE.has(scenario.board);
  await parkPointer(frames);
  // The pointer moves the scenario's own steps made (pointer, point,
  // click); the rest's click on the query field is counted apart.
  let pointerSteps = 0;
  let fieldClick = false;
  if (!authored) {
    // Rest: the query field focused by a real click, as the native fixture
    // opens with its field focused; the pointer is then over the header,
    // never over a row.
    const field = await evaluate(`__wb.outer(${boardIndex}, __wb.frame(${boardIndex}).contentDocument.querySelector('input.q'))`);
    await pointerTo(field.x, field.y);
    await call('Input.dispatchMouseEvent', { type: 'mousePressed', x: field.x, y: field.y, button: 'left', clickCount: 1 });
    await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: field.x, y: field.y, button: 'left', clickCount: 1 });
    fieldClick = true;
    await sleep(300);
  }
  const record = { name: scenario.name, board: scenario.board ?? 'root', authored, captures: [], steps: scenario.steps };
  const after = [];
  const pointerTarget = POINTER_TARGETS[scenario.board];
  for (const step of scenario.steps) {
    const takes = AUTHORED_STEPS[scenario.board];
    const taken = takes && takes.has(step.action) && (step.action !== 'pointer' || pointerTarget);
    if (authored && step.action !== 'capture' && !taken) {
      after.push(step);
      continue;
    }
    if (step.action === 'capture') {
      const state = await evaluate(`__wb.${STATES[scenario.board] ?? 'state'}(${boardIndex})`);
      const frame = frames[boardIndex].frame;
      const glass = (await evaluate('__wb.frames()'))[boardIndex].glass;
      await evaluate(`__wb.fontsReady(${boardIndex})`);
      const shot = await screenshot(join(dir, `${step.name}.png`), frame, glass);
      // The board's faces as captured, and where the pointer was (#103).
      const fonts = await evaluate(`__wb.fontsOf(${boardIndex})`);
      const pointer = { page: pointerAt, board: pointerOnBoard(frame, glass), steps: pointerSteps, fieldClick };
      record.captures.push({ name: step.name, after: [...after], file: `${scenario.name}/${step.name}.png`, width: shot.width, height: shot.height, state, fonts, pointer });
      continue;
    }
    if (['pointer', 'point', 'click'].includes(step.action)) pointerSteps++;
    if (step.action === 'pointer' && authored) {
      // A static board's element at the step's index, from a pixel to its
      // left, as the native side arrives.
      const point = await evaluate(`__wb.outer(${boardIndex}, ${pointerTarget(boardIndex, step.row)})`);
      await pointerTo(point.x + (step.nudge ?? 0) - 1, point.y);
      await pointerTo(point.x + (step.nudge ?? 0), point.y);
    } else if (step.action === 'pointer') {
      // The fixture's row index names a shown row; the reference row with
      // the same title is the same item (both lists hold the same data).
      const titles = (await evaluate(`__wb.state(${rootIndex})`)).rows.map((row) => row.title);
      const title = titles[step.row];
      const point = await evaluate(`__wb.outer(${rootIndex}, __wb.rowFor(${rootIndex}, ${JSON.stringify(title)}))`);
      // A nudge lands a second move over the same row somewhere else, so
      // it is movement.
      // From a pixel to the left, as the native side arrives.
      await pointerTo(point.x + (step.nudge ?? 0) - 1, point.y);
      await pointerTo(point.x + (step.nudge ?? 0), point.y);
    } else if (step.action === 'key') {
      const key = KEYS[step.key];
      if (!key) throw new Error(`unknown key ${step.key}`);
      await call('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...key });
      await call('Input.dispatchKeyEvent', { type: 'keyUp', ...key });
      if (scenario.board === 'clipboard') {
        await sleep(150);
        await evaluate(`__wb.clipboardScroll(${boardIndex})`);
      }
    } else if (step.action === 'click') {
      const target = CLICK_TARGETS[step.target];
      if (!target) throw new Error(`unknown click target ${step.target}`);
      const point = await evaluate(`__wb.outer(${boardIndex}, ${target(boardIndex)})`);
      // From a pixel to the left, as the native side arrives; then pressed.
      await pointerTo(point.x - 1, point.y);
      await pointerTo(point.x, point.y);
      await call('Input.dispatchMouseEvent', { type: 'mousePressed', x: point.x, y: point.y, button: 'left', clickCount: 1 });
      await call('Input.dispatchMouseEvent', { type: 'mouseReleased', x: point.x, y: point.y, button: 'left', clickCount: 1 });
      // The board's logic focuses the panel's search as the panel opens
      // (componentDidUpdate, through a ref), which this board runtime never
      // delivers: the capture focuses it as that logic does, so typing
      // filters the actions, as on the native side.
      await sleep(150);
      await evaluate(`(() => { const field = __wb.frame(${boardIndex}).contentDocument.querySelector('.pop input'); if (field) field.focus({ preventScroll: true }); return !!field; })()`);
    } else if (step.action === 'point') {
      const target = POINT_TARGETS[step.target];
      if (!target) throw new Error(`unknown point target ${step.target}`);
      const point = await evaluate(`__wb.outer(${boardIndex}, ${target(boardIndex)})`);
      // From a pixel to the left, as the native side arrives; no press (a
      // slot's click would launch it and hide the panel).
      await pointerTo(point.x - 1, point.y);
      await pointerTo(point.x, point.y);
    } else if (step.action === 'type') {
      for (const character of step.text) {
        await call('Input.insertText', { text: character });
        await sleep(40);
      }
    }
    after.push(step);
    await sleep(350);
  }
  return record;
}

let failure = null;
try {
  let targets = [];
  for (let attempt = 0; attempt < 75 && !targets.some((t) => t.type === 'page'); attempt++) {
    await sleep(200);
    targets = await fetch(`http://127.0.0.1:${debugPort}/json/list`).then((r) => r.json()).catch(() => []);
  }
  const page = targets.find((target) => target.type === 'page');
  if (!page) throw new Error('Chrome exposed no page target');
  socket = new WebSocket(page.webSocketDebuggerUrl);
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    const done = message.id && waiting.get(message.id);
    if (done) {
      waiting.delete(message.id);
      done(message);
    }
  };
  await new Promise((open, fail) => {
    socket.onopen = open;
    socket.onerror = () => fail(new Error('could not connect to Chrome'));
  });
  manifest.chrome = (await call('Browser.getVersion')).product;
  await call('Page.enable');

  // Every board, at its own size, with Windows labels.
  const frames = await load();
  const rootIndex = frames.findIndex((frame) => BOARDS[0][1].test(frame.title ?? ''));
  if (rootIndex < 0) throw new Error(`the reference has no root board: ${JSON.stringify(frames)}`);
  mkdirSync(join(out, 'boards'), { recursive: true });
  for (const [index, frame] of frames.entries()) {
    const slug = (BOARDS.find(([, pattern]) => pattern.test(frame.title ?? '')) ?? [`board-${index}`])[0];
    const board = { slug, title: frame.title, frame: frame.frame, glass: frame.glass, sourceOnly: SOURCE_ONLY.has(slug), file: null };
    if (frame.glass) {
      const shot = await screenshot(join(out, 'boards', `${slug}.png`), frame.frame, frame.glass);
      board.file = `boards/${slug}.png`;
      board.image = [shot.width, shot.height];
      board.fonts = await evaluate(`__wb.fontsOf(${index})`);
    }
    manifest.boards.push(board);
  }
  manifest.fonts = await evaluate('__wb.fonts()');

  // Every reference-backed scenario from a fresh load, so no state leaks
  // from one scenario into the next.
  const only = args.scenario ? new Set(args.scenario.split(',')) : null;
  for (const scenario of registry.scenarios) {
    if (!scenario.reference || (only && !only.has(scenario.name))) continue;
    const fresh = await load();
    manifest.scenarios.push(await runScenario(scenario, rootIndex, fresh));
    console.log(`reference ${scenario.name}: ${manifest.scenarios.at(-1).captures.length} captures`);
  }
} catch (error) {
  failure = error;
  manifest.error = String(error.stack ?? error);
  console.error(`reference-capture failed: ${error.message}`);
} finally {
  try { socket?.close(); } catch {}
  server.close();
  // End only the Chrome tree this script started, then delete only the
  // profile it created.
  let killed = false;
  try {
    execFileSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    killed = true;
  } catch {
    killed = chrome.exitCode !== null;
  }
  await sleep(500);
  let profileRemoved = false;
  for (let attempt = 0; attempt < 10 && !profileRemoved; attempt++) {
    try {
      rmSync(profile, { recursive: true, force: true });
      profileRemoved = true;
    } catch {
      await sleep(300);
    }
  }
  manifest.cleanup = { chromePid: chrome.pid, chromeEnded: killed, profileRemoved, server: 'closed' };
  manifest.finishedUtc = new Date().toISOString();
  writeFileSync(join(out, 'reference-manifest.json'), JSON.stringify(manifest, null, 2));
}
process.exit(failure ? 1 : 0);
