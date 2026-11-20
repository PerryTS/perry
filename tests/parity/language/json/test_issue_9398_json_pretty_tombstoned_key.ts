// #9398: `JSON.stringify(value, replacer, space)` over an object whose key was
// removed by an O(1) tombstone delete.
//
// The delete writes TAG_HOLE over the KEY slot and leaves the keys array
// length alone. The plain (no-replacer, no-indent) stringify walk skipped that
// hole; the replacer / pretty / array-replacer walks in json/replacer.rs did
// not — they used the raw NaN-box bits of any non-string, non-pointer tag AS A
// POINTER and dereferenced `TAG_HOLE` (0x7FFC_0000_0000_0010) as a
// StringHeader, i.e. SIGSEGV.
//
// `claude mcp remove <name>` is exactly this shape: drop the server key, then
// rewrite ~/.claude.json with a 2-space indent.

function tombstoned(): Record<string, unknown> {
  const o: Record<string, unknown> = { srv1: { command: "/bin/echo", args: ["hi"] } };
  delete o.srv1;
  return o;
}

// The pre-fix crash: indented stringify of a receiver whose only key is a hole.
console.log("pretty own:", JSON.stringify(tombstoned(), null, 2));
console.log("pretty nested:", JSON.stringify({ mcpServers: tombstoned() }, null, 2));

// Same walk reached through a function replacer, and through a replacer array.
console.log(
  "fn replacer:",
  JSON.stringify({ mcpServers: tombstoned(), keep: 1 }, (_k, v) => v),
);
console.log(
  "fn replacer pretty:",
  JSON.stringify({ mcpServers: tombstoned(), keep: 1 }, (_k, v) => v, 2),
);
console.log(
  "array replacer:",
  JSON.stringify({ mcpServers: tombstoned(), keep: 1, drop: 2 }, ["mcpServers", "keep"]),
);
console.log(
  "array replacer pretty:",
  JSON.stringify({ mcpServers: tombstoned(), keep: 1, drop: 2 }, ["mcpServers", "keep"], 2),
);

// A surviving sibling must still serialize, and the deleted key must be gone
// rather than reappearing as `null` / `"fieldN"`.
const partial: Record<string, unknown> = { a: 1, b: 2, c: 3 };
delete partial.b;
console.log("partial pretty:", JSON.stringify(partial, null, 2));
console.log("partial replacer:", JSON.stringify(partial, (_k, v) => v, 2));
console.log("partial keys:", JSON.stringify(Object.keys(partial)));

// Delete-then-readd must not resurrect the tombstone.
const readded: Record<string, unknown> = { x: 1 };
delete readded.x;
readded.y = 2;
console.log("readded pretty:", JSON.stringify(readded, null, 2));

// Deleting every key of a multi-key object.
const emptied: Record<string, unknown> = { p: 1, q: 2 };
delete emptied.p;
delete emptied.q;
console.log("emptied pretty:", JSON.stringify({ w: emptied }, null, 2));
