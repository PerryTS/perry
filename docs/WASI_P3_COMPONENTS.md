# Authoring a WASI 0.3 component

The integration branch exposes P3 guest generation and composition through
`perry-wit`. The [Linux qualification](validation/wasi-public-p3-20261102.json)
executes an async-only plugin through generated TypeScript host bindings and
resource-bearing futures through an independent Rust host. Installed P3
packages for Linux, macOS and Windows are still being qualified; use matching
source-built tools and archives for this workflow.

Use a compiler built with `perry/target-wasi`, its matching native runtime,
stdlib and optional component-host archives, a plain `wasm32-wasip3` runtime
archive, `perry-wit`, and wasi-sdk 34. The native archives also need their
matching GC-effect sidecars. The SDK and native compiler run on the build host;
the resulting component targets WASI 0.3.0.

In a fresh project directory, create `world.wit`:

```wit
package perry:public-async@1.0.0;

world plugin {
  export ping: async func(value: u32) -> u32;
}
```

Generate the adapters, composition plan, guest API and typed host bindings:

```sh
perry-wit --guest-c --target wasip3 world.wit generated plugin
```

Create `guest.ts`:

```ts
import { register } from "./generated/guest";

register({
  async ping(value: number) {
    return value + 1;
  },
});
```

Point the build at the qualified tools and archives. `PERRY_RUNTIME_DIR`
contains the native `.a` archives; `P3_RUNTIME` names the plain P3 runtime `.a`.

```sh
export WASI_SDK_PATH=/tools/wasi-sdk-34.0
export PERRY_RUNTIME_DIR=/artifacts/native
P3_RUNTIME=/artifacts/wasm32-wasip3/libperry_runtime.a
```

Compile the TypeScript implementation and link the producer component:

```sh
perry compile guest.ts --target wasip3 --output-type staticlib \
  --no-auto-optimize --no-cache -o libguest.a

"$WASI_SDK_PATH/bin/clang" --target=wasm32-wasip3 -mexec-model=reactor \
  -O1 -g -Wall -Werror -Wno-unused-function -I generated \
  generated/plugin.c generated/perry_adapter.c generated/plugin_component_type.o \
  -Wl,--whole-archive libguest.a -Wl,--no-whole-archive "$P3_RUNTIME" -lsetjmp \
  -Wl,--export=cabi_realloc -Wl,--stack-first -Wl,-z,stack-size=8388608 \
  -Wl,--fatal-warnings -o producer.component.wasm
```

Link the public facade and compose the final component:

```sh
"$WASI_SDK_PATH/bin/clang" --target=wasm32-wasip3 -mexec-model=reactor \
  -O1 -g -Wall -Werror -I generated \
  generated/perry_producer_facade.c generated/perry_producer_forward.c \
  generated/perry_producer_facade_component_type.o \
  -Wl,--export=cabi_realloc -Wl,--fatal-warnings -o facade.component.wasm

perry-wit compose producer.component.wasm facade.component.wasm \
  generated/perry_producer_facade.json plugin.component.wasm
```

The generated `bindings.ts` loads the component through Perry's native
component engine. Create `host.ts`:

```ts
import { readFileSync } from "node:fs";
import { load } from "./generated/bindings";

async function main() {
  const component = load(readFileSync(process.argv[2]), {}, {
    fuel: 1000000000,
    memoryBytes: 268435456,
    wasi: {},
  });
  const result = await component.exports.ping(41);
  if (result !== 42) throw new Error("unexpected plugin result");
  component.dispose();
  console.log(result);
}
main();
```

```sh
perry compile host.ts --no-auto-optimize --no-cache -o host
./host plugin.component.wasm
# 42
```

Async functions and typed `future<T>`/`stream<T>` transport require the explicit
`--target wasip3` adapter profile. Plain `--guest-c` and `--target wasip2`
retain the synchronous profile. Endpoint payloads cannot transitively contain
`borrow<T>`; invalid declarations fail before creating the output directory.
Nominal resource ownership remains distinct across differently labelled imports
of the same interface. The broader supported value and lifecycle matrix,
providers and installed platform packages are tracked in
[WASI_030_COMPLETION.md](WASI_030_COMPLETION.md).

The maintained gate is `scripts/component_public_p3_acceptance.py`. It rebuilds
this example and a resource-future example through the public commands, checks
ordinary and instrumented execution, requires an actual moving-GC witness and
checks invalid endpoint declarations. Its independent native probes can be
built by the gate or explicitly supplied with `--probe-dir` for recorded reuse.
