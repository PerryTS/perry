`perry/tui` now hands TypeScript real objects instead of small registry
integers. Every value the module returned — a widget from `Text` / `Box` /
`Table` / `Tabs` / …, a `state(initial)` container, a `useRef` box, and the
`useApp()` / `useStdout()` / `useFocusManager()` singletons — used to be an id
NaN-boxed with `POINTER_TAG`, a number pretending to be a pointer. Three
registries minted those ids and three more were plain constants, so SIX id
spaces shared one encoding and they collided:

    useApp()          -> 1     Text("hi")   -> 1     useRef(x), first  -> 1
    useStdout()       -> 2     Box()        -> 2     useRef(y), second -> 2
    useFocusManager() -> 3     Spacer()     -> 3
    state(0), first   -> 0     <- POINTER_TAG | 0, a null pointer wearing
                                  the pointer tag

`useApp() === Text("hi")` was therefore `true`, a `Map` or `Set` keyed on two
different handles kept one entry, a `WeakMap` entry stored under a widget was
readable through the App handle, and the first `state(0)` of a program was a
tagged null. None of it was reachable through a type error: the values are
indistinguishable at run time, because the encoding carries no provenance.

Each kind is now a `GC_TYPE_OBJECT` with its own class id, a real ShapeId and
ZERO own keys, so `typeof` is `"object"`, `Object.keys` is `[]`,
`JSON.stringify` is `{}` (it was `null`), spread and `Object.assign` copy
nothing, and two handles are two values. `useApp()`, `useStdout()` and
`useFocusManager()` still answer the SAME object on every call — ink's do, and
perry's did too while they were constants — so they are per-realm singletons in
rooted slots rather than re-minted per call. `useRef` is likewise stable across
renders: the hook slot owns its handle object.

`state.get()` / `.set(v)`, `ref.get()` / `.set(v)`, `app.exit()` /
`.waitUntilExit()`, `stdout.write()` / `.columns()` / `.rows()` and
`focusManager.focusNext()` / `.focusPrevious()` / `.focus(id)` are now real
methods on a per-kind prototype as well as the statically lowered
`class_filter` rows they already were. Before this they existed ONLY as static
lowerings, so a handle reached through an untyped value (`const s: any = state(0)`)
answered `undefined` for every one of them.

The registry ids are unchanged and stay the module's internal currency: the
widget tree, the Taffy layout pass, the paint pass and the hook slots all still
speak ids, and only the value that crosses the FFI boundary changed. A handle
of one kind can no longer address another kind's registry entry, which the
overlapping id spaces previously allowed — `tui::is_known_handle` and the three
`contains_handle` probes it unioned are deleted, because a class-id load answers
the same question without asking three mutexes.
