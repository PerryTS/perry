import { execFile } from "node:child_process";

const args = ["space value", 'quote"value', "unicode-中文", "", "--flag=value"];
await new Promise<void>((resolve) => {
  execFile(
    "node",
    [
      "-e",
      "process.stdout.write(JSON.stringify(process.argv.slice(1)))",
      ...args,
    ],
    { encoding: "utf8" },
    (error, stdout, stderr) => {
      console.log("error:", error === null ? "null" : error?.name);
      console.log("args:", stdout);
      console.log("stderr:", JSON.stringify(stderr));
      resolve();
    },
  );
});
