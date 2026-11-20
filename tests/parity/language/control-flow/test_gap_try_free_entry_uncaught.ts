// ehfree slice 1 (design §5 tests 8 and 11): with no handler recorded at
// `try` entry, "uncaught" means the unwinder's search found no landing pad
// (`_URC_END_OF_STACK`; the unhandled-exception filter on Windows), with the
// stack still intact. The process must still run its `exit` listeners
// before the report and leave with status 1 (#9403), from the top level, from
// deep recursion and from a timer callback; and a deep-recursion RangeError
// must still land in a `try` far up the stack.
import * as childProcess from "node:child_process";

const marker = "__perry_ehfree_uncaught_child__";
const markerIndex = process.argv.indexOf(marker);

function deep(n: number): number {
  if (n === 0) throw new Error("deep uncaught");
  return deep(n - 1) + 1;
}

function recurse(n: number): number {
  return recurse(n + 1) + 1;
}

if (markerIndex >= 0) {
  const role = process.argv[markerIndex + 1];
  process.on("exit", (code) => {
    console.log(`exit listener code=${code}`);
  });
  if (role === "top") {
    console.log("child top: before throw");
    throw new Error("top uncaught");
  } else if (role === "deep") {
    console.log("child deep: before throw");
    deep(500);
  } else if (role === "timer") {
    setTimeout(() => {
      console.log("child timer: before throw");
      throw new Error("timer uncaught");
    }, 0);
  } else if (role === "caught") {
    try {
      deep(200);
    } catch (e) {
      console.log("child caught:", (e as Error).message);
    }
  }
} else {
  // 8. A stack-overflow RangeError from far below lands in this try.
  let overflow = "none";
  try {
    recurse(0);
  } catch (e) {
    overflow = (e as Error).constructor.name;
  }
  console.log("8 overflow caught as", overflow);
  // Something after the overflow still throws and catches normally.
  try {
    deep(50);
  } catch (e) {
    console.log("8 then:", (e as Error).message);
  }

  const script = process.argv[1];
  const selfArgs = (role: string): string[] =>
    typeof script === "string" && script.endsWith(".ts")
      ? [script, marker, role]
      : [marker, role];
  for (const role of ["top", "deep", "timer", "caught"]) {
    const result = childProcess.spawnSync(process.execPath, selfArgs(role), {
      encoding: "utf8",
    });
    const stdout = (result.stdout ?? "").trim().split("\n").join(" | ");
    const reported = (result.stderr ?? "").includes("uncaught");
    console.log(`11 ${role}: status=${result.status} stdout=[${stdout}] reported=${reported}`);
  }
}
