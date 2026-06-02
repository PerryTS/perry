// Issue #1071: cross-module imported `const` object literal — flat + nested.
// Pre-fix every read from the consumer side returned `undefined` because the
// WASM target lowered the imported name to `ExternFuncRef`, which then fell
// off the end of `func_name_map` and emitted TAG_UNDEFINED. Now the consumer
// resolves the imported name to the source module's let-global.

export interface Theme {
  background: string;
  tokens: { keyword: string };
}

export const DARK_THEME: Theme = {
  background: '#1e1e1e',
  tokens: { keyword: '#569cd6' },
};

export const VERSION = 'v1';
export const ANSWER = 42;
