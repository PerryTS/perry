// A pending asynchronous write lands before a natural exit.
import { writeFile } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
writeFile(join(tmpdir(), "nmi-exit-teardown-write.out"), "written before exit\n", (err) => {
  console.log(err ? "failed" : "callback");
});
console.log("queued");
