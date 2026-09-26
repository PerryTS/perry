# Hello World

## Your First Program

Create a file called `hello.ts`:

```typescript
{{#include ../../examples/getting-started/hello.ts}}
```

Compile and run it:

```bash
perry hello.ts -o hello
./hello
```

Output:

```
Hello, Perry!
```

That's it. Perry compiled your TypeScript to a native executable — no Node.js, no bundler, no runtime.

## A Slightly Bigger Example

```typescript
{{#include ../../examples/getting-started/fibonacci.ts}}
```

```bash
perry fib.ts -o fib
./fib
```

This runs about 2x faster than Node.js because Perry compiles to native machine code with integer specialization.

## Using Variables and Functions

```typescript
{{#include ../../examples/getting-started/snippets.ts:variables-functions}}
```

## Async Code

```typescript
{{#include ../../examples/getting-started/async_fetch.ts}}
```

```bash
perry fetch.ts -o fetch
./fetch
```

Perry compiles async/await to native code driven by its own event loop (turnloop).

## Multi-Threading

Perry can do something no JavaScript runtime can — run your code on multiple CPU cores:

```typescript
{{#include ../../examples/getting-started/multi_threading.ts}}
```

This is real OS-level parallelism, not web workers or separate isolates. See [Multi-Threading](../threading/overview.md) for details.

## What the Compiler Produces

When you run `perry file.ts -o output`, Perry:

1. Parses your TypeScript with SWC
2. Lowers the AST to an intermediate representation (HIR)
3. Applies optimizations (inlining, closure conversion, etc.)
4. Generates native machine code with LLVM
5. Links with your system's C compiler

The result is a standalone executable with no external dependencies.

### Binary Size

| Program | Binary Size |
|---------|-------------|
| Hello world | ~300KB |
| CLI with fs/path | ~3MB |
| UI app | ~3MB |
| Full app with stdlib | ~48MB |

Perry automatically detects which runtime features you use and only links what's needed.

## Next Steps

- [Build a native UI app](first-app.md)
- [Configure your project](project-config.md)
- [Explore supported TypeScript features](../language/supported-features.md)
