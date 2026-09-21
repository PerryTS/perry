/**
 * `@perryts/react` — TypeScript types for Perry's React support.
 *
 * Two jobs:
 *
 *  1. **Integrate with the app's React types.** This package accepts React 18
 *     and React 19 type packages as peers so the app chooses the matching
 *     major.
 *
 *  2. **Type Perry's `createRoot` extension.** This file augments
 *     `react-dom/client` with the native-window overload
 *     `createRoot(null, { title, width, height })`.
 *
 * Activate it by adding the package to your tsconfig `types` (recommended, so
 * the augmentation is always in scope):
 *
 * ```jsonc
 * // tsconfig.json
 * { "compilerOptions": { "jsx": "react-jsx", "types": ["@perryts/react"] } }
 * ```
 *
 * …or import it once anywhere in your project:
 *
 * ```ts
 * import '@perryts/react';
 * import { createRoot } from 'react-dom/client';
 * createRoot(null, { title: 'Counter', width: 300, height: 200 }).render(<Counter />);
 * ```
 *
 * NOTE: the `createRoot` augmentation only takes effect when the file that
 * holds it is loaded directly (via `types: [...]`, a direct `import`, or a
 * `/// <reference>`). TypeScript drops module augmentations reached only
 * indirectly through a re-export from another `.d.ts`, so the augmentation
 * lives here in the package entry rather than in a re-exported sub-module.
 */

/// <reference types="react" />
/// <reference types="react-dom" />

import type { Root } from 'react-dom/client';

/**
 * Options for the native window that hosts a Perry React root, passed as the
 * second argument to `createRoot(null, options)`.
 *
 * `title` / `width` / `height` are the documented core; the rest mirror the
 * common `perry/ui` window knobs and are all optional.
 */
export interface PerryRootOptions {
  /** Window title shown in the title bar. */
  title?: string;
  /** Initial content width, in points. */
  width?: number;
  /** Initial content height, in points. */
  height?: number;
  /** Minimum content width the user can resize to, in points. */
  minWidth?: number;
  /** Minimum content height the user can resize to, in points. */
  minHeight?: number;
  /** Whether the user can resize the window. Defaults to `true`. */
  resizable?: boolean;
  /** Initial window x position, in screen points. Centered if omitted. */
  x?: number;
  /** Initial window y position, in screen points. Centered if omitted. */
  y?: number;
}

declare module 'react-dom/client' {
  /**
   * Perry native-window overload: pass `null` (no DOM container) plus the
   * native window options. Standard `react-dom`'s DOM-container overloads stay
   * available — this only adds the `null` form. Returns the same `Root` so
   * `.render()` / `.unmount()` work as usual.
   */
  export function createRoot(container: null | undefined, options: PerryRootOptions): Root;
}
