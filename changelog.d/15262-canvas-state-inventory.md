Internal: the canvas draw-command tables of the native UI backends are recorded in the GC root-holder inventory as holding no GC pointer, which the Windows build's custody audit requires.
