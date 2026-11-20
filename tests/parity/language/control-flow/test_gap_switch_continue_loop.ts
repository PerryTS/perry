// `continue` inside a `switch` inside a loop must jump to the LOOP's continue
// target — never fall into the post-switch tail (which only `break` arms
// reach). The switch lowering pushed its exit label as BOTH the break and
// continue target, so `continue` compiled exactly like `break`: every case arm
// ending in `continue` executed the code after the switch with stale locals.
//
// react-server-dom's flight row parser is a for-loop state machine whose case
// arms end in `continue`; each row-boundary step fell into the row-commit tail
// with a stale slice index, mis-framing every RSC row Next.js streamed (#5989).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// (1) basic: case-0's continue must skip the tail; only case-1's break reaches it
function t1(): string {
  let hits = 0;
  const log: string[] = [];
  for (let i = 0; i < 4; ) {
    switch (i) {
      case 0:
        i = 1;
        log.push("c0");
        continue;
      case 1:
        i = 2;
        log.push("c1");
        break;
    }
    hits++;
    log.push(`tail(i=${i})`);
    i = 4;
  }
  return `hits=${hits} ${log.join(",")}`;
}
console.log(t1());

// (2) a var written before the switch (the parser's `d` pattern): the tail must
// only ever observe the break arm's value
function t2(): string {
  const log: string[] = [];
  for (let i = 0, guard = 0; i < 10 && guard < 8; guard++) {
    let d = -1;
    switch (i % 2) {
      case 0:
        d = 100 + i;
        i++;
        log.push(`even d${d}`);
        continue;
      case 1:
        d = 7;
        i++;
        break;
    }
    log.push(`tail d${d} i${i}`);
  }
  return log.join(",");
}
console.log(t2());

// (3) while-loop + default arm with continue
function t3(): string {
  const log: string[] = [];
  let i = 0;
  let guard = 0;
  while (i < 6 && guard++ < 10) {
    switch (i) {
      case 0:
        i = 2;
        log.push("a");
        continue;
      case 2:
        i = 4;
        log.push("b");
        break;
      default:
        i = 6;
        log.push("c");
        continue;
    }
    log.push("tail" + i);
  }
  return log.join(",");
}
console.log(t3());

// (4) the flight-parser shape: byte state machine over a Uint8Array, rows
// delimited by ':' and '\n' — every framing step continues; the commit tail
// runs only via the case-with-break
function t4(): string {
  const bytes = new Uint8Array([49, 58, 104, 105, 10, 50, 58, 106, 10]); // "1:hi\n2:j\n"
  const rows: string[] = [];
  let iter = 0;
  for (var n = 0, a = 0, o = 0, c = bytes.length; n < c; ) {
    if (++iter > 50) return "STUCK";
    var d = -1;
    switch (a) {
      case 0:
        58 === (d = bytes[n++]) ? (a = 1) : (o = (o << 4) | (d - 48));
        continue;
      case 1:
        d = bytes.indexOf(10, n);
        break;
    }
    if (-1 < d) {
      rows.push(`${o}:${d - n}`);
      n = d + 1;
      o = a = 0;
    } else break;
  }
  return rows.join(",");
}
console.log(t4());

// (5) continue inside switch inside a LABELED outer loop via plain continue
function t5(): string {
  const log: string[] = [];
  outer: for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      switch (j) {
        case 0:
          log.push(`i${i}j${j}`);
          continue; // inner loop
        case 1:
          continue outer;
      }
      log.push("unreachable-tail");
    }
  }
  return log.join(",");
}
console.log(t5());
