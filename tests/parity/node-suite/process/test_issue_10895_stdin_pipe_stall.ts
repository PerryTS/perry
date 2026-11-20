// #10895: `for await (const chunk of process.stdin)` on a pipe stalled forever
// part-way through the input. The async iterator pauses the source after every
// delivered chunk and resumes it on the next pull; `pause()` makes the fd-0
// reader thread exit and `resume()` respawns it, and a resume that landed while
// the old reader was still on its way out lost the respawn for good.
//
// Driven by `crates/perry/tests/issue_10895_stdin_pipe_stall.rs`, which pipes
// several MiB in small writes. Undriven (the parity sweep) it must not touch
// stdin at all: the sweep inherits whatever stdin the caller has, and a fixture
// that waits for EOF on a terminal never finishes.
const driven = process.env.PERRY_10895_DRIVE === "1";

async function main(): Promise<void> {
  if (!driven) {
    console.log("RESULT:idle");
    return;
  }
  let total = 0;
  for await (const data of process.stdin) {
    const chunk: Uint8Array = data;
    total += chunk.length;
    // Touch the payload so a chunk that arrives with the right length but the
    // wrong bytes is caught too (every byte the driver writes is 1).
    if (chunk[0] !== 1 || chunk[chunk.length - 1] !== 1) {
      console.log("RESULT:corrupt@" + total);
      return;
    }
  }
  console.log("RESULT:" + total);
}

main();
