# Linux install-preview regression captures

Unmodified PNGs from `gui-smoke-Linux` artifacts. `package.png` was
`preview-wait.png` of
[CI run 40150674331](https://github.com/pane-app/pane/actions/runs/40150674331)
(source `973249e4`, after #529 gave the preview its top bar); `root.png` was
`1-root.png` of
[CI run 36951745142](https://github.com/hoangvu12/pane/actions/runs/36951745142)
(source `b15cbdb`). These are dark/opaque Linux Xvfb captures at 760 by 460
panel pixels, surrounded by the 1280 by 800 virtual desktop.

The package preview is visibly ready. The top bar's back button, a filled
rounded square where root search draws only its magnifier, counts 538
pixels of the button's gray in the sampling box (Windows counted 519 in the
same run, under a resting pointer's hover wash), over the 192-pixel minimum.
The root capture counts 9. (The package's title beside the button was too
thin a sign: Linux drew 8 to 17 pixels of it.) The regression tests require
package acceptance and root rejection; no desktop session or application
launch is needed.
