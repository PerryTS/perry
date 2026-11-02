Compile OpenTUI's Solid universal JSX ahead of time using the nearest
`tsconfig.json` `jsxImportSource`, or an explicit `perry.jsx.runtime`. Preserve
the `"solid"` and `"default"` modes. Native JSX now emits static properties,
tracked property effects, text nodes, insertion markers, refs and directives;
components preserve dynamic getters, render props and ordered spreads.

Select Solid's reactive core/store builds throughout the compilation graph and
honor OpenTUI's Bun entry when `--platform bun` is enabled. Configuration cache
inputs include per-source and inherited JSX settings. Add runtime-selection
tests and a pinned differential fixture using OpenTUI's real Babel transform,
Solid's universal renderer and OpenTUI's character-frame renderer.
