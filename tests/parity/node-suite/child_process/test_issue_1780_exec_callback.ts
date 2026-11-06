// Issue #1780: child_process.exec(cmd[, options], callback) was a null stub —
// the callback never fired (codegen lowered it then discarded it). Now exec
// runs the command and invokes the callback with (err, stdout, stderr).
// Chained so the ordering is deterministic across Node (deferred callbacks)
// and Perry (immediate callbacks).
import { exec } from "child_process";

exec("echo first", (err: any, stdout: string, stderr: string) => {
  console.log("1 err:", err, "out:", stdout.trim(), "errOut:", JSON.stringify(stderr));
  exec("echo second", { encoding: "utf8" }, (err2: any, stdout2: string) => {
    console.log("2 out:", stdout2.trim());
    exec("exit 4", (err3: any) => {
      console.log("3 err is null:", err3 === null);
    });
  });
});
