Fixed macOS `perry/ui` colours rendering in Generic RGB instead of sRGB (#11608).
The web target emits CSS `rgba()`, which CSS Color 4 defines as sRGB, so the
same RGBA value rendered differently on macOS: `#00C2FF` showed as `#00CDFF`.
Two calls in `perry-ui-macos` caused the shift: `CGColorCreateGenericRGB` for
layer backgrounds, gradients, borders, and shadows, and `colorWithCalibratedRed:`
in the chart and command palette. A new `srgb` module now creates every
`NSColor`, `CGColor`, and canvas/chart fill and stroke colour in sRGB. The
`colorWithRed:` and `CGContextSetRGB{Fill,Stroke}Color` sites already painted
sRGB pixels; they move to the module for consistency and change no output.
