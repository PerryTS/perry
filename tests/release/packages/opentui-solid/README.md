Compile-time Solid universal JSX regression for #10099, using OpenTUI 0.4.5
(the OpenCode v1.18.30 renderer) and its declared Solid 1.9.12 peer version.

`fixture.sh` compares a native executable with the official OpenTUI transform.
The headless host uses the real `solid-js/universal` renderer and exercises
signal/store updates, static identifiers, property effects, mixed child ranges,
component getters, refs, directives, spread precedence, fragments, control flow,
and disposal. `tsconfig.json` selects the dialect automatically; a fixture alias
routes only the renderer host to `host.ts`.

For differential expansion tests and actual OpenTUI character frames:

```sh
cargo build -p perry-hir --example solid_jsx --profile perry-dev
python tests/release/packages/opentui-solid/compare.py \
  target/perry-dev/examples/solid_jsx \
  --perry target/perry-dev/perry \
  --opencode /path/to/opencode-v1.18.30
```

The optional OpenCode argument checks every TSX file in both source trees with
the real transform and Perry's ordinary HIR lowering. It checks for residual
JSX runtime calls; it does not execute OpenCode's full worker/native-addon graph.
That application's first native TUI frame also depends on #10100, #10103, #10105.
