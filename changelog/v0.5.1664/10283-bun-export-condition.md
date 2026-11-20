Rank the `bun` condition above `node` when resolving package `exports` and
`imports` under `--platform bun`. Bun resolves with `["bun", "node", …]`, so a
package that ships both entries — `@opentui/core` offers `{ bun:
./index.bun.js, node: ./index.node.js, import: ./index.node.js }` — previously
compiled its node entry and ran a different backend than the bun binary the
program is meant to match. OpenCode's terminal interface died on exactly that:
the node entry's renderer backend is not the one its bun build loads. `bun`
stays below `perry`, so an explicit perry entry still wins, and the order is
unchanged for every other target. The three resolvers now read one shared
list so they cannot disagree.
