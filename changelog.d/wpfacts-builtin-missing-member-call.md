### Fixes

- Whole-program facts call a missing built-in member through its ordinary field read, preserving program stores, accessors, receiver binding and prototype changes. A missing function no longer exposes its arguments to unknown code. Native function `__proto__` assignments now retain the same prototype relation as `Object.setPrototypeOf`.
