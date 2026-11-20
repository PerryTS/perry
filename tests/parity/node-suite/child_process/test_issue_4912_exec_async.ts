// Issue #4912: child_process.exec/execFile must run off the main thread and
// fire their callbacks on a *later* event-loop tick — not synchronously, as the
// previous buffered implementation did. Compared byte-for-byte vs
// `node --experimental-strip-types`.
import { exec, execFile } from "node:child_process";

// 1. Ordering: the callback must fire AFTER the synchronous code that follows
//    the exec() call. With the old synchronous-callback stub this printed
//    "exec cb" before "after exec()".
let phase = "sync";
exec("echo hello", (err: any, stdout: string) => {
  console.log("exec cb err:", err, "stdout:", stdout.trim(), "phase:", phase);

  // 2. execFile is async too.
  execFile("/bin/echo", ["world"], (err2: any, stdout2: string) => {
    console.log("execFile cb err:", err2, "stdout:", stdout2.trim());

    // 3. Error shape is preserved on the async path.
    exec("exit 3", (err3: any) => {
      console.log("exec exit3 code:", err3 && err3.code);
      console.log("exec exit3 cmd:", err3 && err3.cmd);
      console.log("exec exit3 keys:", err3 ? Object.keys(err3).sort().join(",") : "");
    });
  });
});
phase = "async";
console.log("after exec()");
