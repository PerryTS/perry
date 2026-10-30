The inliner copies content-addressed `__AnonShape_*` classes into every module
that inlines a body using them, so each copy defined the same program-global
layout descriptor symbols. Only the first module owning a family now defines
them and the others declare them external.
