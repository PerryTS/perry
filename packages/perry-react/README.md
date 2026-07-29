# @perryts/react

TypeScript types for [Perry](https://github.com/PerryTS/perry)'s React support.

Perry compiles React/JSX to **native widgets** — standard React components
render to a native macOS/iOS/Android app, with no DOM and no browser. The
runtime lives in [`perry-react`](https://github.com/PerryTS/react) and
[`perry-react-dom`](https://github.com/PerryTS/react-dom); **this package is the
type layer** for it.

It does two things:

1. **Integrates with the app's React types.** It accepts both the React 18 and
   React 19 type packages as peers, so the app remains in control of the type
   major matching its installed React version.

2. **Types Perry's `createRoot` extension.** `@types/react-dom` only knows the
   DOM-container form `createRoot(domNode)`. Perry has no DOM, so it extends
   `createRoot` to take `null` plus native window options. This package augments
   `react-dom/client` with that overload, so the call below type-checks instead
   of erroring on the `null` container / extra argument.

## Install

```bash
npm install -D @perryts/react
# Install one matching major of the runtime and type peers:
npm install react react-dom
npm install -D @types/react @types/react-dom
```

## Use

Add it to your tsconfig so the `createRoot` augmentation is always in scope:

```jsonc
// tsconfig.json
{
  "compilerOptions": {
    "jsx": "react-jsx",
    "types": ["@perryts/react"]
  }
}
```

…or import it once anywhere in your project:

```tsx
import '@perryts/react';
import { useState } from 'react';
import { createRoot } from 'react-dom/client';

function Counter() {
  const [n, setN] = useState(0);
  return <button onClick={() => setN(n + 1)}>Count: {n}</button>;
}

// Perry overload: null container + native window options — now fully typed.
createRoot(null, { title: 'Counter', width: 300, height: 200 }).render(<Counter />);
```

### `PerryRootOptions`

The second argument to `createRoot(null, …)`. `title` / `width` / `height` are
the documented core; the rest mirror the common `perry/ui` window knobs and are
optional:

```ts
import type { PerryRootOptions } from '@perryts/react';

const opts: PerryRootOptions = {
  title: 'My App',
  width: 900,
  height: 640,
  minWidth: 400,
  resizable: true,
};
```

## License

MIT
