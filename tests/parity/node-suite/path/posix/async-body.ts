// The path functions answer the same in an async function body as in a synchronous one.
import * as path from "node:path";

async function inAsync(): Promise<string[]> {
  return [
    path.sep,
    path.delimiter,
    path.normalize("/a//b/../c/."),
    path.join("/a", "b", "../c"),
    path.resolve("/a", "b", "../c"),
    path.relative("/a/b/c", "/a/d"),
    path.basename("/x/file.txt", ".txt"),
    path.dirname("/x/y/z"),
    path.extname("f.tar.gz"),
    String(path.isAbsolute("/a")),
    JSON.stringify(path.parse("/home/u/file.txt")),
    path.format({ dir: "/x", base: "y.js" }),
    path.toNamespacedPath("/a/b"),
    String(path.matchesGlob("src/a/b.ts", "src/**/*.ts")),
    path.win32.join("C:\\a", "b"),
    path.win32.resolve("C:\\a", "b"),
    JSON.stringify(path.win32.parse("C:\\a\\b.txt")),
    String(path.win32.matchesGlob("a\\b\\c", "a/**/c")),
  ];
}

inAsync().then((lines) => {
  for (const line of lines) console.log(line);
});
