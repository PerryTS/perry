import { createRequire } from 'node:module';
import { load } from './_helpers/node_modules/sf1-require-failures/loader.mjs';
// Supply the package's ambient require with the same module-relative base as
// its compiled synchronous require glob. The loader has no local require alias.
(globalThis as any).require = createRequire(new URL('./_helpers/node_modules/sf1-require-failures/loader.mjs', import.meta.url));
for (const name of ['bad', 'throws']) {
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const value = load(name);
      console.log(name, attempt, 'returned', typeof value);
    } catch (error: any) {
      console.log(name, attempt, error.name);
    }
  }
}
