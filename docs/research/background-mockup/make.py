"""Builds index.html: the reference launcher over a user background image,
treated the way Roboco treats its new-thread background, with live controls,
so the look can be judged before any GPUI work.

Run: python make.py, then open index.html.

The wallpapers it was built from (wallhaven downloads under src/ and their
derived imgs/) are not kept in the repository, so the committed index.html
shows broken pictures until src/ is filled again; the shot-*.png files are
captures of the judged look.

It reads the reference's root board from docs/research/ui-port/reference/,
which is no longer in the repository (ADR 0029 retired the reference
comparison); to run it again, restore that folder from the archive tag:
git checkout archive/impl-ui-91-2026-10-06 -- docs/research/ui-port/reference
(https://github.com/hoangvu12/pane/tree/archive/impl-ui-91-2026-10-06/docs/research/ui-port/reference)."""
import colorsys
import re
from pathlib import Path

from PIL import Image, ImageFilter

HERE = Path(__file__).parent
REF = HERE.parents[2] / "docs/research/ui-port/reference"
SOURCE = Path(r"C:/Users/ADMIN/Downloads/wallhaven-yqkwg7.jpg")

SOURCES = [SOURCE, *sorted((HERE / "src").glob("wallhaven-*"))]
(HERE / "imgs").mkdir(exist_ok=True)


def dominant(img):
    """Roboco's wallpaper_colors::extract: a 4-bit-per-channel histogram,
    each pixel weighted by 0.2 + saturation^2, the heaviest bin's mean."""
    small = img.copy()
    small.thumbnail((64, 64))
    bins = {}
    for r, g, b in small.getdata():
        _, s, _ = colorsys.rgb_to_hsv(r / 255, g / 255, b / 255)
        w = 0.2 + s * s
        key = (r >> 4, g >> 4, b >> 4)
        acc = bins.setdefault(key, [0.0, 0.0, 0.0, 0.0])
        acc[0] += r * w
        acc[1] += g * w
        acc[2] += b * w
        acc[3] += w
    r, g, b, w = max(bins.values(), key=lambda acc: acc[3])
    return r / w, g / w, b / w


def toward(color, target, amount):
    return tuple(round(c + (t - c) * amount) for c, t in zip(color, target))


def dark_canvas(color):
    """Keeps a matched canvas a dark panel: lightness 6–16%, saturation at
    most 45%, so the dark palette's text keeps its contrast on it."""
    h, l, s = colorsys.rgb_to_hls(*(c / 255 for c in color))
    rgb = colorsys.hls_to_rgb(h, min(max(l, 0.06), 0.16), min(s, 0.45))
    return tuple(round(c * 255) for c in rgb)


# Pane's dark panel today, and the panel moved 55% toward the wallpaper's
# dominant colour (Roboco's 0.88-toward-black tint lands near black on an
# already dark wallpaper; its own canvas reads as a navy around #262a3a).
canvas_plain = (22, 23, 26)

# Where the launcher's view of the wallpaper sits: cover-fit into 760x518,
# focused 50% across and 45% down, as .bg-img draws it.
VIEW = (760, 518)
IMAGE_OPACITY = 0.84


def luminance(rgb):
    """WCAG relative luminance of an sRGB colour in 0-255."""
    def channel(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (channel(c) for c in rgb)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def mix(a, b, t):
    """a moved t of the way toward b, as CSS composites: in sRGB."""
    return tuple(x + (y - x) * t for x, y in zip(a, b))


def view_of(img):
    w, h = img.size
    scale = max(VIEW[0] / w, VIEW[1] / h)
    cw, ch = VIEW[0] / scale, VIEW[1] / scale
    left, top = (w - cw) * 0.5, (h - ch) * 0.45
    # The top 60%: where the fade still shows the image strongly.
    return img.crop((round(left), round(top), round(left + cw), round(top + ch * 0.6)))


def percentile_colour(img, p):
    pixels = sorted(img.getdata(), key=luminance)
    return pixels[min(len(pixels) - 1, int(len(pixels) * p))]


def least(alpha_from, backdrop, canvas, target):
    """The smallest alpha of `canvas` over `backdrop` whose result has a
    luminance at most `target`."""
    alpha = alpha_from
    while alpha < 0.95 and luminance(mix(backdrop, canvas, alpha)) > target:
        alpha += 0.01
    return round(alpha, 2)


def contrast(img, canvas):
    """Per wallpaper, measured where the launcher shows it:

    - the dim over the image, so the raw image's bright 75th percentile —
      what the section labels sit on — darkens to luminance 0.15 (the
      brightened labels then keep about 3:1);
    - the frost tint, so the *blurred* image's bright 90th percentile under
      a frosted surface darkens to luminance 0.08 (text above 7:1, the
      brightened secondary text near 4:1).

    Dark wallpapers need neither and stay as they are."""
    view = view_of(img)
    small = view.copy()
    small.thumbnail((190, 190))
    shown = lambda c: mix(c, canvas, 1 - IMAGE_OPACITY)
    raw = shown(percentile_colour(small, 0.75))
    dim = least(0.0, raw, canvas, 0.15)
    blurred = small.filter(ImageFilter.GaussianBlur(6))
    under = mix(shown(percentile_colour(blurred, 0.90)), canvas, dim)
    frost = least(0.15, under, canvas, 0.08)
    return frost, dim
# Each wallpaper as Pane would keep it — the source downscaled once — with
# its own matched canvas and a thumbnail for the picker.
WALLS = []
for source in SOURCES:
    name = source.stem.replace("wallhaven-", "")
    out = HERE / "imgs" / f"{name}.jpg"
    thumb = HERE / "imgs" / f"{name}-t.jpg"
    image = Image.open(source).convert("RGB")
    if not out.exists():
        big = image.copy()
        big.thumbnail((1920, 1920), Image.LANCZOS)
        big.save(out, quality=88)
        small = image.copy()
        small.thumbnail((160, 90), Image.LANCZOS)
        small.save(thumb, quality=80)
    kept = Image.open(out).convert("RGB")
    canvas = dark_canvas(toward(canvas_plain, dominant(kept), 0.55))
    frost, dim = contrast(kept, canvas)
    WALLS.append((name, canvas, frost, dim))
    print(name, canvas, "frost", frost, "dim", dim)
canvas_match = WALLS[0][1]

html = (REF / "root.html").read_text(encoding="utf8")
start = html.index("<section")
end = html.index("</section>", start) + len("</section>")
section = re.sub(r' data-dc-tpl="\d+"', "", html[start:end])
# The image layers, under everything the panel holds.
section = section.replace(
    ">",
    '><div class="hero"><div class="bg-img"></div><div class="bg-sharp"></div><div class="bg-scan"></div><div class="bg-dim"></div></div>',
    1,
)
section = section.replace('<div style="height: 64px;', '<div class="hdr" style="height: 64px;', 1)
section = section.replace('<div style="height: 50px;', '<div class="ftr" style="height: 50px;', 1)
css = (REF / "root.css").read_text(encoding="utf8")

# A smoothstep-shaped fade from --fs% to --fe% of the panel's height.
def stop(alpha, t):
    return f"rgba(0,0,0,{alpha}) calc((var(--fs) + (var(--fe) - var(--fs)) * {t}) * 1%)"


fade = ", ".join(
    [stop(1, 0), stop(0.9, 0.2), stop(0.65, 0.4), stop(0.35, 0.6), stop(0.1, 0.8), stop(0, 1)]
)

page = f"""<!doctype html>
<html><head><meta charset="utf-8"><title>Pane background mockup</title>
<style>
{css}
body{{min-height:100vh;display:flex;flex-direction:column;align-items:center;gap:24px;padding:24px;
  background:#15171c}}
.desk{{position:relative;width:1100px;height:640px;border-radius:14px;overflow:hidden;display:flex;
  justify-content:center;align-items:flex-start;padding-top:60px;
  background:linear-gradient(135deg,#3a4150,#262a33 55%,#4b4f5c)}}
:root{{--ho:1;--hh:58;--ps:1;--sy:0;--pb:0px;--be:100;--canvas:{",".join(map(str, canvas_match))};--fs:20;--fe:78;--op:.84;--dim:0;
  --fb:16px;--ft:.15;--scan:1}}
.glass{{background:rgb(var(--canvas))!important;-webkit-backdrop-filter:none!important;
  backdrop-filter:none!important}}
.glass>*:not(.hero){{position:relative;z-index:1}}
/* The hero: the image's box, the top --hh% of the panel, moving up as the
   list scrolls (--sy is the list's scroll offset times --ps). */
.hero{{position:absolute;left:0;right:0;top:0;height:calc(var(--hh) * 1%);overflow:hidden;
  transform:translateY(calc(var(--sy) * -1px));opacity:var(--ho);will-change:transform,opacity}}
.bg-img,.bg-sharp,.bg-scan{{position:absolute;inset:0;-webkit-mask-image:linear-gradient(180deg,{fade});
  mask-image:linear-gradient(180deg,{fade})}}
/* Progressive blur: the blurred image everywhere, the sharp one over it at
   the top, fading out from --be minus 24% to --be. */
.bg-img,.bg-sharp{{background:50% 45%/cover no-repeat;opacity:var(--op)}}
.bg-img{{filter:blur(var(--pb));transform:scale(calc(1 + var(--pb) / 300px))}}
.bg-sharp{{-webkit-mask-image:linear-gradient(180deg,{fade}),linear-gradient(180deg,#000 calc((var(--be) - 24) * 1%),transparent calc(var(--be) * 1%));
  mask-image:linear-gradient(180deg,{fade}),linear-gradient(180deg,#000 calc((var(--be) - 24) * 1%),transparent calc(var(--be) * 1%));
  -webkit-mask-composite:source-in;mask-composite:intersect}}
.walls{{width:1100px;display:flex;gap:6px;flex-wrap:wrap}}
.walls img{{width:80px;height:45px;object-fit:cover;border-radius:6px;cursor:pointer;opacity:.7;
  box-shadow:0 0 0 1px rgba(255,255,255,.1)}}
.walls img:hover{{opacity:1}}
.walls img.on{{opacity:1;box-shadow:0 0 0 2px #c9ee6a}}
.bg-scan{{opacity:var(--scan);background:repeating-linear-gradient(180deg,
  rgba(0,0,0,.48) 0 1px,transparent 1px 3px)}}
.bg-dim{{position:absolute;inset:0;background:rgba(var(--canvas),var(--dim));
  -webkit-mask-image:linear-gradient(180deg,{fade});mask-image:linear-gradient(180deg,{fade})}}

/* Frost: Roboco's composer pill — a 16px backdrop blur under a thin tint of
   the canvas (85% of the blurred image shows), with a cool silver edge. */
.frost{{background:rgba(var(--canvas),var(--ft));-webkit-backdrop-filter:blur(var(--fb));
  backdrop-filter:blur(var(--fb));box-shadow:inset 0 0 0 1px hsla(210,18%,78%,.09),
  inset 0 1px 0 rgba(255,255,255,.06)}}
.hdr{{border-bottom-color:hsla(210,18%,78%,.09)!important}}
body.pill .hdr{{height:52px!important;margin:12px 12px 6px;border-radius:16px;border-bottom:0!important;
  padding:0 16px!important}}
body.pill .hdr .q{{height:52px}}
body.pill .list{{height:398px!important}}
.slot{{background:rgba(var(--canvas),var(--ft))}}
.slot,.row.sel,.ftr{{-webkit-backdrop-filter:blur(var(--fb));backdrop-filter:blur(var(--fb))}}
.slot{{box-shadow:inset 0 0 0 1px hsla(210,18%,78%,.09),inset 0 1px 0 rgba(255,255,255,.06)}}
.slot:hover{{background:rgba(255,255,255,.09)}}
.row:hover{{background:rgba(255,255,255,.06)}}
.row.sel{{background:rgba(255,255,255,.10);box-shadow:inset 0 0 0 1px hsla(210,18%,78%,.10)}}
.ftr{{background:rgba(var(--canvas),calc(var(--ft) + .2))!important;
  border-top-color:hsla(210,18%,78%,.09)!important}}
body.today .hero{{display:none}}
body.today .glass{{background:linear-gradient(180deg,rgba(255,255,255,.05),rgba(255,255,255,0) 36%),
  rgba(22,23,26,.7)!important;backdrop-filter:blur(44px) saturate(160%)!important}}
body.today .frost,body.today .slot,body.today .row.sel,body.today .ftr{{backdrop-filter:none}}
/* Over an image, the secondary text steps up, as Roboco hardens its foregrounds. */
body:not(.today) .label{{color:#c9cace}}
body:not(.today) .q::placeholder{{color:#a9aaaf}}
body:not(.today) .row-s,body:not(.today) .row-kind{{color:#a9aaaf}}

.panel{{width:1100px;display:grid;grid-template-columns:repeat(2,1fr);gap:9px 28px;
  color:#d9dadd;font-size:13px}}
.row2{{grid-column:1/-1;display:flex;gap:8px;flex-wrap:wrap;align-items:center}}
.row2 button{{padding:7px 12px;border-radius:8px;background:rgba(255,255,255,.08);
  box-shadow:inset 0 0 0 1px rgba(255,255,255,.1)}}
.row2 button.on{{background:#c9ee6a;color:#111}}
.row2 label{{display:flex;gap:6px;align-items:center;margin-left:10px}}
label.s{{display:grid;grid-template-columns:150px 1fr 54px;align-items:center;gap:10px}}
.swatch{{display:inline-block;width:14px;height:14px;border-radius:4px;vertical-align:middle;
  background:rgb({",".join(map(str, canvas_match))})}}
</style></head>
<body class="pill">
<div class="panel">
  <div class="row2">
    <button data-p="hero">Hero (scrolls away)</button>
    <button data-p="prog">Progressive blur</button>
    <button data-p="roboco">Roboco</button>
    <button data-p="clean">Roboco, no scanlines</button>
    <button data-p="whole">Whole image, frosted UI</button>
    <button data-p="today">Today (glass, no image)</button>
    <label><input type="checkbox" data-t="pill" checked>Search as a frosted pill</label>
    <label><input type="checkbox" data-t="auto">Auto contrast</label>
    <label><input type="checkbox" data-t="dissolve" checked>Dissolve on scroll</label>
    <label><input type="checkbox" data-t="match" checked>Match wallpaper colors <span class="swatch"></span></label>
  </div>
  <label class="s">Fade starts (%)<input type="range" min="0" max="100" data-v="fs"><span></span></label>
  <label class="s">Fade ends (%)<input type="range" min="10" max="160" data-v="fe"><span></span></label>
  <label class="s">Image opacity<input type="range" min="0" max="1" step=".01" data-v="op"><span></span></label>
  <label class="s">Dim over image<input type="range" min="0" max=".9" step=".01" data-v="dim"><span></span></label>
  <label class="s">Frost blur<input type="range" min="0" max="44" data-v="fb" data-u="px"><span></span></label>
  <label class="s">Frost tint<input type="range" min="0" max=".9" step=".01" data-v="ft"><span></span></label>
  <label class="s">Scanlines<input type="range" min="0" max="1" step=".05" data-v="scan"><span></span></label>
  <label class="s">Image height (%)<input type="range" min="30" max="100" data-v="hh"><span></span></label>
  <label class="s">Scroll speed<input type="range" min="0" max="1.5" step=".05" data-v="ps"><span></span></label>
  <label class="s">Sharp area ends (%)<input type="range" min="24" max="124" data-v="be"><span></span></label>
  <label class="s">Lower blur<input type="range" min="0" max="60" data-v="pb" data-u="px"><span></span></label>
</div>
<div class="walls">{"".join(f'<img src="imgs/{name}-t.jpg" data-w="{i}" title="{name}">' for i, (name, *_) in enumerate(WALLS))}</div>
<div class="desk">{section}</div>
<script>
let current=0;
const P={{
  hero:{{hh:100,ps:1.25,fs:0,fe:105,op:.56,dim:.64,fb:30,ft:.4,scan:.5,be:73,pb:60}},
  prog:{{hh:100,ps:0,fs:30,fe:118,op:.84,dim:0,fb:24,ft:.15,scan:.5,be:46,pb:28}},
  roboco:{{hh:100,ps:0,fs:20,fe:78,op:.84,dim:0,fb:24,ft:.15,scan:1,be:124,pb:0}},
  clean:{{hh:100,ps:0,fs:20,fe:78,op:.84,dim:0,fb:24,ft:.15,scan:0,be:124,pb:0}},
  whole:{{hh:100,ps:0,fs:100,fe:160,op:.84,dim:.35,fb:24,ft:.15,scan:0,be:124,pb:0}},
  today:{{fs:20,fe:78,op:0,dim:0,fb:16,ft:.15,scan:0}},
}};
const CANVAS={{match:'{",".join(map(str, canvas_match))}',plain:'{",".join(map(str, canvas_plain))}'}};
const root=document.documentElement.style;
const inputs=[...document.querySelectorAll('input[data-v]')];
function show(i){{const u=i.dataset.u||'';root.setProperty('--'+i.dataset.v,i.value+u);
  i.nextElementSibling.textContent=i.value+u}}
function set(k,v){{const i=inputs.find(i=>i.dataset.v==k);i.value=v;show(i)}}
inputs.forEach(i=>i.addEventListener('input',()=>show(i)));
document.querySelector('.hdr').classList.add('frost');
const pill=document.querySelector('[data-t=pill]'),match=document.querySelector('[data-t=match]');
pill.onchange=()=>document.body.classList.toggle('pill',pill.checked);
match.onchange=()=>root.setProperty('--canvas',match.checked?CANVAS.match:CANVAS.plain);
document.querySelectorAll('.row2 button').forEach(b=>b.onclick=()=>{{
  document.querySelectorAll('.row2 button').forEach(x=>x.classList.toggle('on',x==b));
  document.body.classList.toggle('today',b.dataset.p=='today');
  Object.entries(P[b.dataset.p]).forEach(([k,v])=>set(k,v));
  if(typeof applyAuto=='function')applyAuto();
}});
document.querySelector('[data-p='+(location.hash.slice(1)||'hero')+']').click();
// The wallpapers: each with its own matched canvas. Click a thumbnail, or
// press [ and ] to step through them.
const WALLS={[[name, ",".join(map(str, canvas)), frost, dim] for name, canvas, frost, dim in WALLS]};
const auto=document.querySelector('[data-t=auto]');
// The measured tint and dim of the wallpaper shown; the whole-image preset keeps
// at least its own dim.
function applyAuto(){{if(!auto.checked||document.body.classList.contains('today'))return;
  const [,,frost,dim]=WALLS[current];set('ft',frost);
  const whole=document.querySelector('[data-p=whole]').classList.contains('on');
  set('dim',whole?Math.max(dim,.35):dim)}}
auto.onchange=applyAuto;
function wall(i){{
  current=(i+WALLS.length)%WALLS.length;
  const [name,canvas]=WALLS[current];
  document.querySelectorAll('.bg-img,.bg-sharp').forEach(l=>l.style.backgroundImage='url(imgs/'+name+'.jpg)');
  CANVAS.match=canvas;
  document.querySelector('.swatch').style.background='rgb('+canvas+')';
  match.onchange();
  applyAuto();
  document.querySelectorAll('.walls img').forEach(t=>t.classList.toggle('on',+t.dataset.w==current));
}}
document.querySelectorAll('.walls img').forEach(t=>t.onclick=()=>wall(+t.dataset.w));
document.addEventListener('keydown',e=>{{if(e.key=='[')wall(current-1);if(e.key==']')wall(current+1)}});
wall(0);
// The hero follows the list's scroll.
const list=document.querySelector('.list');
// It also dissolves as it goes, gone once the list has scrolled half its
// height, so no faded strip is left along the top edge.
const dissolve=document.querySelector('[data-t=dissolve]');
function follow(){{
  const css=getComputedStyle(document.documentElement);
  root.setProperty('--sy',list.scrollTop*parseFloat(css.getPropertyValue('--ps')));
  const height=document.querySelector('.glass').clientHeight*parseFloat(css.getPropertyValue('--hh'))/100;
  root.setProperty('--ho',dissolve.checked?Math.max(0,1-list.scrollTop/(height*.5)):1)}}
dissolve.onchange=follow;
list.addEventListener('scroll',follow);
inputs.forEach(i=>i.addEventListener('input',follow));
// Selection follows clicks, as the launcher's does.
document.querySelectorAll('.row').forEach(r=>r.addEventListener('click',()=>{{
  document.querySelectorAll('.row').forEach(x=>x.classList.toggle('sel',x==r))}}));
</script>
</body></html>"""
(HERE / "index.html").write_text(page, encoding="utf8")
print("wrote", HERE / "index.html")
