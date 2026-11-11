// Literal strings: `Log.last` receives an append accumulator, a uniquely
// owned string, so the fact does not hold and the store demotes it to
// shared. Under PERRY_FACTS_SABOTAGE=litstr every field and local that may
// hold a String is claimed to hold only literals' values: the store would
// drop the demote, and the append after it would rewrite the stored string
// in place. The check-mode witness at the store must report the unshared
// string ("literal string").
class Log {
  last = "none";
  run(n: number): number {
    let line = "start";
    for (let i = 0; i < n; i++) {
      line += "x";
      if (i === 20) this.last = line;
    }
    line += "-tail";
    return line.length;
  }
}
const log = new Log();
const total = log.run(40);
console.log(log.last, log.last.length, total);
