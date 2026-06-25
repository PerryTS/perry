# Project Configuration

Perry projects use `perry.toml` and `package.json` for configuration. No special config file is required for basic usage, but larger projects benefit from Perry-specific settings.

> **Looking for the full perry.toml reference?** See [perry.toml Reference](../cli/perry-toml.md) for every field, section, platform option, and environment variable.

## Basic Setup

```bash
perry init my-project
cd my-project
```

This scaffolds `perry.toml`, `package.json`, a starter `src/main.ts`, `.gitignore`, and `tsconfig.json` (plus Perry type stubs under `.perry/types/`).

## package.json

The generated `package.json` carries the npm-interop layer. The `perry.compilePackages` array is seeded empty — it is the sole config home for that setting (it is read only from `package.json`, never from `perry.toml`):

```json
{
  "name": "my-project",
  "version": "0.1.0",
  "private": true,
  "main": "src/main.ts",
  "perry": {
    "compilePackages": []
  }
}
```

### Perry Configuration

The `perry` field in `package.json` controls compiler behavior:

#### `compilePackages`

List npm packages to compile natively instead of routing through the JavaScript runtime:

```json
{
  "perry": {
    "compilePackages": ["@noble/curves", "@noble/hashes"]
  }
}
```

> **Two-key opt-in.** Listing a package is not sufficient on its own. Compiling
> third-party TypeScript into your binary is a privileged operation, so every
> entry in `compilePackages` must *also* be matched by an entry in
> `perry.allow.compilePackages` in the same `package.json` — otherwise the build
> refuses. Patterns accept exact names, scope wildcards (`"@scope/*"`), or the
> universal `"*"`:
>
> ```json
> {
>   "perry": {
>     "compilePackages": ["@noble/curves", "@noble/hashes"],
>     "allow": { "compilePackages": ["@noble/*"] }
>   }
> }
> ```
>
> For one-off builds where editing `package.json` isn't an option, set
> `PERRY_ALLOW_PERRY_FEATURES=1` to opt every name in (and `=0` to force the
> refusal even when `package.json` opted in).

When a package is listed here, Perry:
1. Resolves the package in `node_modules/`
2. Prefers TypeScript source (`src/index.ts`) over compiled JavaScript (`lib/index.js`)
3. Compiles all functions natively through LLVM
4. Deduplicates across nested `node_modules/` to prevent duplicate linker symbols

This is useful for pure TypeScript/JavaScript packages that don't rely on Node.js APIs. Packages that use native bindings, `eval()`, or dynamic `require()` won't work.

#### `codegen`

Perry is an ahead-of-time compiler: it never runs a code string at runtime. Many libraries that would normally JIT a function from a schema or a config (`ajv`, `fast-json-stringify`, Prisma, Drizzle, …) ship a **build-time** mode that emits plain, eval-free source instead. The `codegen` field declares the commands that produce that source. Perry runs them **before** compiling, then compiles the generated output natively — so the shipped binary links no JavaScript engine.

```json
{
  "perry": {
    "codegen": [
      { "label": "ajv validators", "command": "node scripts/generate-validators.mjs" }
    ]
  }
}
```

Each entry is either a bare command string or an object with `command` (required) and an optional `label` shown in build output. Commands run in declaration order, with the working directory set to the folder containing this `package.json`, so relative script paths resolve as expected. If a command exits non-zero the build fails and prints its captured stdout/stderr.

**Security:** `codegen` is read **only** from the host project's `package.json` — never from a dependency's — so a transitive dependency can't smuggle in a build command (the same trust boundary as `compilePackages`). Skip the steps for a reproducible or sandboxed build (where the generated output is already committed) with `perry compile --no-codegen` or `PERRY_SKIP_CODEGEN=1`.

##### Worked example: `ajv/standalone`

`ajv` validates against a JSON Schema. Its default mode JITs the validator with `new Function`; its **standalone** mode emits the same validator as plain source. The generator script:

```js
// scripts/generate-validators.mjs
import Ajv from "ajv";
import standaloneCode from "ajv/dist/standalone/index.js";
import { writeFileSync } from "node:fs";

const schema = {
  $id: "Config",
  type: "object",
  properties: { host: {}, port: {} },
  required: ["host", "port"],
  additionalProperties: false,
};

const ajv = new Ajv({ code: { source: true } }); // standalone source
const moduleCode = standaloneCode(ajv, ajv.compile(schema));
writeFileSync(new URL("../generated/validator.cjs", import.meta.url), moduleCode);
```

Then import the generated validator like any other module:

```ts,no-test
import validate from "./generated/validator.cjs";
if (!validate(input)) throw new Error("invalid config");
```

`perry compile` runs the `codegen` step, ajv emits `generated/validator.cjs` (no `new Function`), and Perry compiles it natively. See `test-files/test_ajv_standalone.ts` for a runnable, byte-parity-tested sample.

##### Same convention, other tools

The convention is library-agnostic — point a `codegen` command at any build-time generator and import its output:

| Tool | `command` | Output to import |
|------|-----------|------------------|
| **ajv** | `node scripts/generate-validators.mjs` (uses `ajv/standalone`) | generated validator module |
| **Prisma** | `prisma generate` | generated client |
| **Drizzle** | `drizzle-kit introspect` | generated schema/types |
| **kysely-codegen** | `kysely-codegen --out-file src/db.d.ts` | generated DB types |
| **Vue SFC** | `vue-tsc` / your SFC compile step | compiled `.vue` output |

Libraries that JIT at runtime with **no** standalone mode (e.g. `fast-json-stringify`, `find-my-way`) are handled separately — see the [`eval` / `new Function` strategy](https://github.com/PerryTS/perry/issues/1677).

#### `splash`

Configure a native splash screen for iOS and Android. The splash screen appears instantly during cold start, before your app code runs.

**Minimal (both platforms share the same splash):**

```json
{
  "perry": {
    "splash": {
      "image": "logo/icon-256.png",
      "background": "#FFF5EE"
    }
  }
}
```

**Per-platform overrides:**

```json
{
  "perry": {
    "splash": {
      "image": "logo/icon-256.png",
      "background": "#FFF5EE",
      "ios": {
        "image": "logo/splash-ios.png",
        "background": "#FFFFFF"
      },
      "android": {
        "image": "logo/splash-android.png",
        "background": "#FFFFFF"
      }
    }
  }
}
```

**Full custom override (complete control):**

```json
{
  "perry": {
    "splash": {
      "ios": {
        "storyboard": "splash/LaunchScreen.storyboard"
      },
      "android": {
        "layout": "splash/splash_background.xml",
        "theme": "splash/themes.xml"
      }
    }
  }
}
```

| Field | Description |
|-------|-------------|
| `splash.image` | Path to a PNG image, centered on the splash screen (both platforms) |
| `splash.background` | Hex color for the background (default: `#FFFFFF`) |
| `splash.ios.image` | iOS-specific image override |
| `splash.ios.background` | iOS-specific background color |
| `splash.ios.storyboard` | Custom LaunchScreen.storyboard (compiled with ibtool) |
| `splash.android.image` | Android-specific image override |
| `splash.android.background` | Android-specific background color |
| `splash.android.layout` | Custom drawable XML for `windowBackground` |
| `splash.android.theme` | Custom themes.xml |

**Resolution order** per platform:
1. Custom file override (storyboard / layout+theme)
2. Platform-specific image/color (`splash.{platform}.image`)
3. Universal image/color (`splash.image`)
4. No `splash` key → blank white screen (backward compatible)

## Using npm Packages

Perry natively supports many popular npm packages without any configuration:

```typescript
{{#include ../../examples/getting-started/npm_packages.ts}}
```

These are compiled to native code using Perry's built-in implementations. See [Standard Library](../stdlib/overview.md) for the full list.

For packages not natively supported, use `compilePackages` for pure TS/JS packages, or the JavaScript runtime fallback for complex packages.

## Project Structure

Perry is flexible about project structure. Common patterns:

```
my-project/
├── package.json
├── src/
│   └── index.ts
└── node_modules/      # Only needed for compilePackages
```

For UI apps:

```
my-app/
├── package.json
├── src/
│   ├── index.ts       # Main app entry
│   └── components/    # UI components
└── assets/            # Images, etc.
```

## Compilation

```bash
# Compile a file
perry src/index.ts -o build/app

# Compile with a specific target
perry src/index.ts -o build/app --target ios-simulator

# Debug: print intermediate representation
perry src/index.ts --print-hir
```

See [CLI Commands](../cli/commands.md) for all options.

## Next Steps

- [CLI Commands](../cli/commands.md) — All compiler commands and flags
- [Supported Features](../language/supported-features.md) — What TypeScript features work
- [Standard Library](../stdlib/overview.md) — Supported npm packages
