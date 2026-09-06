# Solid for Perry native UI

`perry-solid` connects Solid's universal renderer to Perry's native widget
handles. Signals update existing widgets directly. The renderer keeps parent
and sibling information in TypeScript so keyed lists can move native widgets
without recreating them.

This is the runtime bridge from [#4644](https://github.com/PerryTS/perry/issues/4644).
It provides native hyperscript and an opt-in Solid JSX compiler mode.
Solid's bundled `solid-js/h` and `solid-js/html` use its web renderer and are
not substitutes for this package's `h`.

## Use from this checkout

In an application project, install the local package and Solid:

```sh
npm install /path/to/perry/packages/perry-solid solid-js@1.9.15
```

Select Solid's reactive client runtime in the application's `package.json`:

```json
{
  "perry": {
    "compilePackages": ["solid-js"],
    "allow": { "compilePackages": ["solid-js"] },
    "packageAliases": {
      "solid-js": "solid-js/dist/solid.js",
      "solid-js/store": "solid-js/store/dist/store.js"
    }
  }
}
```

These aliases also apply inside Solid's universal renderer and stores, so they
share the same reactive owner. Solid's default Node entry is an intentionally
nonreactive server build. `perry-solid` declares `nativeModule: true`; its
TypeScript is compiled natively along with Solid.

```ts
import { App, VStack } from "perry/ui";
import { createSignal } from "solid-js";
import { h, render } from "perry-solid";

const body = VStack([]);
const dispose = render(() => {
  const [count, setCount] = createSignal(0);
  return h("VStack", { padding: 16 },
    h("Text", { fontSize: 24 }, () => `Count: ${count()}`),
    h("Button", { onPress: () => setCount(n => n + 1) }, "Increment"),
  );
}, body);

App({ title: "Solid + Perry", width: 400, height: 240, body });
// Call dispose() when unmounting this root: it stops effects and detaches nodes.
```

[examples/counter.ts](examples/counter.ts) adds a keyed list and a rotate button.
Compile it from the package directory with:

```sh
perry examples/counter.ts -o counter
./counter
```

## Components and properties

### JSX

Set `"jsx": "solid"` inside your application's `perry` configuration alongside
the Solid client aliases above. The equivalent TOML setting is `[perry]` with
`jsx = "solid"`. An omitted setting, or `"default"`, keeps Perry's existing JSX
behavior. Perry performs the transform in the compiler; Babel is only used as
an independent test oracle for this package.

```tsx
import { createSignal } from "solid-js";
import { For } from "perry-solid";

function Counter() {
  const [count, setCount] = createSignal(0);
  return <vstack padding={16}>
    <text fontSize={24}>Count: {count()}</text>
    <button onPress={() => setCount(value => value + 1)}>Increment</button>
  </vstack>;
}
```

Native intrinsic tags are `vstack`, `hstack`, `text`, `button`, `spacer`, and
`divider`. Capitalized and member names refer to your components. Use
`<For each={items()}>{item => <text>{item.name}</text>}</For>` for keyed lists.
Signal reads in properties and children stay reactive; a signal write updates
the affected widgets without rerunning the component. References support a
callback or an assignable variable/member, and reference callbacks run untracked.
Spreads keep property precedence, and fragments group native children.

For TypeScript checking, use `"jsx": "preserve"` and
`"jsxImportSource": "perry-solid"` in `tsconfig.json`. Perry consumes the `.tsx`
source directly. [examples/counter.tsx](examples/counter.tsx) is a complete app;
copy it into the application project where you installed `perry-solid`, then run
`perry counter.tsx -o counter` there. Generated JSX imports resolve the installed
`perry-solid` package just like handwritten imports.

### Hyperscript

Use `h(Component, props)` for functions returning native children. Reactive
children are accessors (`() => count()`); reactive properties are getters:

```ts
h("Text", { get opacity() { return dimmed() ? 0.5 : 1; } }, "Status")
```

Supported elements are `VStack`, `HStack`, `Text`, `Button`, `Spacer`, and
`Divider`. Stacks use an initial spacing of eight points. `Text` and `Button`
accept text children (including arrays and reactive text); stacks accept
widgets and text. A primitive text child gets its own native Text widget only
when inserted into a stack.

| Property | Native behavior |
| --- | --- |
| `text` | Set a Text value or Button title; use this or text children. |
| `onPress` | Button callback; a reactive getter can replace it. |
| `width`, `height` | Fixed native dimensions. |
| `opacity`, `hidden`, `disabled` | Native widget state. |
| `padding`, `cornerRadius` | Uniform padding and corner radius. |
| `backgroundColor` | Four numeric RGBA channels: `[r, g, b, a]`. |
| `tooltip` | Native tooltip text. |
| `fontSize` | Text font size. |

Properties map to native setters, not CSS. Unsupported element/property names
throw. `ref` follows Solid's spread contract and receives a `NativeNode`; its
`handle` is an opaque Perry widget handle, not an ordinary serializable number.

Import `For` from `perry-solid` for Solid's keyed list behavior with native
child types:

```ts
h("VStack", null, For({
  get each() { return items(); },
  children: item => h("Text", null, item.name),
}))
```

Mount with `render(component, emptyStackHandle)`. Use a dedicated empty native
VStack or HStack; the renderer owns its mounted child order. The returned disposer is
idempotent, runs Solid cleanup, releases stored user callbacks, and detaches
the mounted nodes. Native
widget allocation and reclamation otherwise follow Perry's widget registry.

The low-level universal helpers (`createElement`, `createTextNode`, `insertNode`, `insert`,
`spread`, `setProp`, `createComponent`, `effect`, `memo`, `mergeProps`, and `use`)
are also exported. `perry-solid/renderer` exposes `createNativeRenderer` and its
`NativeDriver` interface for testing host behavior without a display server.

## Validation

```sh
npm ci --ignore-scripts
npm test
npm run test:jsx:oracle
npm run typecheck
PERRY_BIN=/absolute/path/to/perry ../../tests/release/packages/_harness.sh --filter perry-solid
```

The release fixture copies the actual package sources and pinned dependencies,
then checks the same assertions in Node's browser condition and Perry. It
covers reactive properties/text, callback replacement, keyed identity/order,
reparenting, invalid tree operations, and disposal; it also requires zero
JavaScript modules in the native build. The JSX fixture runs the same assertions
through Solid's pinned official Babel universal transform in Node and through
Perry's own JSX compiler. It also checks conditional identity, refs, fragments,
component execution counts, and generated-helper name collisions.

`test/native-smoke.ts` is a real widget app for Geisterhand checks. The macOS
backend's `native_widget_order` Cargo target runs on the main thread and checks
actual AppKit ordering, moves between stacks, retained dimensions, and hidden
reattachment. Other backends use their existing native insertion/removal APIs;
this change does not establish executed platform coverage outside macOS.

From this package directory, with a Geisterhand-enabled Perry installation:

```sh
perry compile test/native-smoke.ts --geisterhand-port 19764 -o /tmp/perry-solid-smoke
python3 test/native-smoke.py /tmp/perry-solid-smoke --output-dir /tmp/perry-solid-smoke-results
```

The runner checks updates to the same native Text handles, button callbacks,
keyed row order with retained widget identities, and stopped effects after
disposal. It saves screenshots and widget snapshots, then exits the app cleanly.
GC scheduling and verifier environment variables are inherited by the app.
To exercise JSX against the same assertions, copy `test/native-smoke.tsx` into
your application project with `perry-solid` installed and `perry.jsx` set to
`"solid"`. Compile that copy with the same Geisterhand flag, then run the Python
runner against the resulting binary.

The client runtime's separate GC verifier correction is in
[#9822](https://github.com/PerryTS/perry/pull/9822). Use that correction for
`PERRY_GC_VERIFY_EVACUATION=1` when testing workloads with retained array-growth
aliases.
