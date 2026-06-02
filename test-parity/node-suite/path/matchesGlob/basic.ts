import * as path from "node:path";

try {
  console.log("txt matches:", path.matchesGlob("foo.txt", "*.txt"));
  console.log("js mismatch:", path.matchesGlob("foo.js", "*.txt"));
  console.log("nested matches:", path.matchesGlob("src/foo.ts", "src/*.ts"));
} catch (e) {
  console.log("matchesGlob ERROR:", (e as Error).message);
}
