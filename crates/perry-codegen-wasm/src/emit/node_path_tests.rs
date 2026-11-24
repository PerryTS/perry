//! `node:path` on the web target (`__nodePath` in `wasm_runtime.js`).

use super::run_web_source;

// The posix functions, the constants and `matchesGlob`, in a synchronous and in an
// async body. `resolve` and `relative` resolve against `process.cwd()`, which is `/`
// on the web target. Expected output is Node's with `/` as its working directory.
#[test]
fn posix_path_functions_answer_like_node() {
    assert_eq!(
        run_web_source(
            r#"
import * as path from "path";
function sync(): string {
  return [
    path.sep, path.delimiter,
    path.normalize("/a//b/../c/."), path.join("a", "", "../b/", "c"),
    path.resolve("/a", "b", "../c"), path.resolve("x/y", "/r", "z"), path.resolve("rel"),
    path.relative("/a/b/c", "/a/d"), path.relative("/a", "/a"),
    path.basename("/x/file.txt", ".txt"), path.basename("/x/dir/"), path.dirname("/x/y/"),
    path.extname("a/.bashrc"), path.extname("f.tar.gz"),
    path.isAbsolute("/a") + "", path.isAbsolute("a") + "",
    JSON.stringify(path.parse("/home/u/file.txt")), JSON.stringify(path.parse("./.env")),
    path.format({ dir: "/x", base: "y.js" }), path.format({ root: "/", name: "n", ext: "e" }),
    path.toNamespacedPath("/a/b"),
    path.matchesGlob("src/a/b.ts", "src/**/*.ts") + "", path.matchesGlob("x/.hidden", "x/*") + "",
    path.matchesGlob("README.md", "*.{md,txt}") + "", path.matchesGlob("f3", "f{1..3}") + "",
  ].join("|");
}
async function inAsync(): Promise<string> {
  return [path.sep, path.normalize("a/./b/.."), path.resolve("/a", "b"), path.relative("/a/b", "/c"),
    JSON.stringify(path.parse("/p/q.r")), path.format({ dir: "d", name: "n", ext: ".e" }),
    path.basename("/x/y.ts", ".ts"), path.isAbsolute("/q") + "", path.matchesGlob("a/b", "a/*") + ""].join("|");
}
console.log(sync());
inAsync().then((v) => console.log("async " + v));
"#
        ),
        concat!(
            r#"/|:|/a/c|b/c|/a/c|/r/z|/rel|../../d||file|dir|/x||.gz|true|false|"#,
            r#"{"root":"/","dir":"/home/u","base":"file.txt","ext":".txt","name":"file"}|"#,
            r#"{"root":"","dir":".","base":".env","ext":"","name":".env"}|/x/y.js|/n.e|/a/b|true|false|true|true"#,
            "\n",
            r#"async /|a|/a/b|../../c|{"root":"/","dir":"/p","base":"q.r","ext":".r","name":"q"}|d/n.e|y|true|true"#
        )
    );
}

// `path.win32`, in a synchronous and in an async body. Expected output is Node's.
#[test]
fn win32_path_functions_answer_like_node() {
    assert_eq!(
        run_web_source(
            r#"
import * as path from "path";
function sync(): string {
  return [
    path.win32.join("C:\\a", "b", "..\\c"), path.win32.dirname("C:\\a\\b.txt"), path.win32.basename("C:\\a\\b.txt"),
    path.win32.basename("C:\\a\\b.txt", ".txt"), path.win32.extname("C:\\a\\b.txt"),
    path.win32.isAbsolute("C:\\a") + "", path.win32.isAbsolute("C:a") + "",
    path.win32.normalize("C:\\a\\..\\b"), path.win32.normalize("\\\\server\\share\\x\\..\\y"),
    JSON.stringify(path.win32.parse("C:\\a\\b.txt")), path.win32.format({ dir: "C:\\x", base: "y.js" }),
    path.win32.relative("C:\\a\\b", "C:\\a\\c"), path.win32.relative("C:\\A\\b", "c:\\a\\B\\d"),
    path.win32.resolve("C:\\a"), path.win32.resolve("C:\\a", "b", "..\\c"), path.win32.resolve("C:\\a", "D:\\b"),
    path.win32.toNamespacedPath("C:\\a"), path.win32.toNamespacedPath("\\\\srv\\share\\f"),
    path.win32.matchesGlob("C:\\a\\b.ts", "C:\\a\\*.ts") + "", path.win32.matchesGlob("foo\\bar", "foo/*") + "",
  ].join("|");
}
async function inAsync(): Promise<string> {
  return [path.win32.join("C:\\a", "b"), path.win32.resolve("C:\\a", "b"),
    JSON.stringify(path.win32.parse("D:\\x\\y.z")), path.win32.matchesGlob("a\\b\\c", "a/**/c") + ""].join("|");
}
console.log(sync());
inAsync().then((v) => console.log("async " + v));
"#
        ),
        concat!(
            r#"C:\a\c|C:\a|b.txt|b|.txt|true|false|C:\b|\\server\share\y|"#,
            r#"{"root":"C:\\","dir":"C:\\a","base":"b.txt","ext":".txt","name":"b"}|C:\x\y.js|..\c|d|"#,
            r#"C:\a|C:\a\c|D:\b|\\?\C:\a|\\?\UNC\srv\share\f|true|true"#,
            "\n",
            r#"async C:\a\b|C:\a\b|{"root":"D:\\","dir":"D:\\x","base":"y.z","ext":".z","name":"y"}|true"#
        )
    );
}

// A non-string argument throws Node's ERR_INVALID_ARG_TYPE TypeError.
#[test]
fn path_rejects_a_non_string_argument_like_node() {
    assert_eq!(
        run_web_source(
            r#"
import * as path from "path";
for (const f of [() => path.join("a", 1 as any), () => path.format(null as any), () => path.matchesGlob("a", 5 as any)]) {
  try { f(); console.log("no throw"); } catch (e: any) { console.log(e.name + " " + e.code + " " + e.message); }
}
"#
        ),
        concat!(
            "TypeError ERR_INVALID_ARG_TYPE The \"path\" argument must be of type string. Received type number (1)\n",
            "TypeError ERR_INVALID_ARG_TYPE The \"pathObject\" argument must be of type object. Received null\n",
            "TypeError ERR_INVALID_ARG_TYPE The \"pattern\" argument must be of type string. Received type number (5)"
        )
    );
}

// The module and its sub-namespaces used as values: a local alias, `apply` / `call`, a
// spread call, identity, a default import, named imports used as values, and `join` with
// two arguments on a namespace value. Expected output is Node's.
#[test]
fn path_module_as_a_value_answers_like_node() {
    assert_eq!(
        run_web_source(
            r#"
import * as path from "path";
import def from "path";
import { join, win32 } from "path";
const w = path.win32;
const p = path.posix;
const args = ["/a", "b"];
console.log([w.join("C:\\a", "b"), p.join("a", "b"), typeof path.join, path.join.apply(null, ["a", "b"]), path.join.call(null, "x", "y"), path.join(...args), path.resolve(...args)].join("|"));
console.log([path.posix === path.posix.posix, path.win32 === def.win32, def.sep, def.basename("/q/r.s", ".s")].join("|"));
async function inAsync(): Promise<string> { const v = path.win32; return v.basename("C:\\x\\y.z") + "|" + path.dirname.apply(null, ["/m/n"]); }
inAsync().then((v) => console.log("async " + v));
const j = join;
console.log([j("a", "b"), join(...args), win32.sep, (w as any)["join"]("C:\\x", "y"), def.win32.join("p", "q")].join("|"));
"#
        ),
        concat!(
            r#"C:\a\b|a/b|function|a/b|x/y|/a/b|/a/b"#,
            "\n",
            r#"true|true|/|r"#,
            "\n",
            r#"a/b|/a/b|\|C:\x\y|p\q"#,
            "\n",
            "async y.z|/m"
        )
    );
}
