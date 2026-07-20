import { exec } from "node:child_process";

await new Promise<void>((resolve) => {
  exec(
    `node -e "process.stdout.write('exec-out');process.stderr.write('exec-err')"`,
    { encoding: "utf8" },
    (error, stdout, stderr) => {
      console.log("error:", error === null ? "null" : error?.name);
      console.log("stdout:", stdout);
      console.log("stderr:", stderr);
      console.log("stdout type:", typeof stdout);
      console.log("stderr type:", typeof stderr);
      resolve();
    },
  );
});
