# Linux install-preview regression captures

Unmodified PNGs from `gui-smoke-Linux` artifacts. `package.png` was
`preview-wait.png` of
[CI run 40150674331](https://github.com/pane-app/pane/actions/runs/40150674331)
(source `973249e4`, after #529 gave the preview its top bar); `root.png` was
`1-root.png` of
[CI run 36951745142](https://github.com/hoangvu12/pane/actions/runs/36951745142)
(source `b15cbdb`). These are dark/opaque Linux Xvfb captures at 760 by 460
panel pixels, surrounded by the 1280 by 800 virtual desktop.

The package preview is visibly ready. Its title, beside the top bar's back
button, counts 17 pixels nearest the title color in the sampling band (Linux
draws its glyphs thin; Windows counted 43 in the same run), over the
10-pixel minimum. The root capture, whose band holds only the search field's
placeholder in the hint color, counts zero. The regression tests require
package acceptance and root rejection; no desktop session or application
launch is needed.
