### fix(gtk4): initialize GTK before creating Canvas

`Canvas()` now initializes GTK before constructing its `DrawingArea`, matching
the other GTK widget constructors. GTK4 apps can therefore create a canvas
before calling `App()` without aborting with “GTK has not been initialized.”
Fixes #7995.
