// Issue #1779: fs.readdir(path, callback) (2-arg form) halted execution — the
// callback closure was passed to readdir_sync as the `options` argument, so it
// read garbage and the program stopped before the callback fired. readdir now
// disambiguates options-vs-callback like the stat/lstat callbacks do.
import * as fs from "fs";

const dir = "/tmp/perry_issue_1779_dir";
fs.rmSync(dir, { recursive: true, force: true });
fs.mkdirSync(dir, { recursive: true });
fs.writeFileSync(dir + "/a.txt", "");
fs.writeFileSync(dir + "/b.txt", "");

// path + callback (the form that previously halted).
fs.readdir(dir, (err: any, files: string[]) => {
  console.log("readdir(path,cb):", err, files.sort().join(","));

  // path + options + callback.
  fs.readdir(dir, { withFileTypes: true }, (err2: any, entries: any[]) => {
    const names = entries.map((e) => e.name).sort().join(",");
    console.log("readdir(path,opts,cb):", err2, names);
    fs.rmSync(dir, { recursive: true, force: true });
  });
});
