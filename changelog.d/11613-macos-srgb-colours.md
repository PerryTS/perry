Fixed macOS `perry/ui` colours rendering in Generic RGB instead of sRGB (#11608).
`perry-ui-macos` built layer colours with `CGColorCreateGenericRGB` and some
`NSColor`s with `colorWithCalibratedRed:`, so `#00C2FF` showed as `#00CDFF` and
did not match the web target. A new `srgb` module now creates every `NSColor`,
`CGColor`, and canvas/chart fill and stroke colour in the sRGB space. The
canvas and chart setters set the colour space and components on the context,
so a redraw allocates no `CGColor`.
