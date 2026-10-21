A compile package subpath import (`@pkg/mini/tool`) now resolves through the
package.json `exports` map, as Node and Bun do, before guessing
`src/<subpath>`. A sibling file named like the subpath (`src/mini/tool.ts`
next to the exported `src/mini/tool.public.ts`) no longer captures the import.
OpenCode v2.1.2 failed to link on this.
