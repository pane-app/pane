# Linux install-preview regression captures

Unmodified PNGs from `gui-smoke-Linux` artifact `11204177862` of
[CI run 36951745142](https://github.com/hoangvu12/pane/actions/runs/36951745142),
source `b15cbdb`. `package.png` was `preview-wait.png`; `root.png` was
`1-root.png`. These are dark/opaque Linux Xvfb captures at 760 by 460 panel
pixels, surrounded by the 1280 by 800 virtual desktop.

The package preview is visibly ready. Its metadata sampling band contains
only 11 pixels within the old exact-color distance of 4, below the unchanged
20-pixel minimum. The existing antialias-aware text matcher counts 137 pixels
nearest the details role in that same band. The root capture counts zero.
The regression tests require package acceptance and root rejection; no desktop
session or application launch is needed.
