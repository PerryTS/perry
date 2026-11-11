// Literal strings: `Log.last` receives an append accumulator, a uniquely
// owned string, so the fact does not hold and the store demotes it to
// shared. Under PERRY_FACTS_SABOTAGE=litstr every field and local that may
// hold a String is claimed to hold only literals' values: the store would
// drop the demote, and the append after it would rewrite the stored string
// in place. The check-mode witness at the store must report the unshared
// string ("literal string").
class Log {
  last = "none";
  keep(s: string): void {
    this.last = s;
  }
}
const log = new Log();
let line = "start";
for (let i = 0; i < 40; i++) {
  line += "x";
  if (i === 20) log.keep(line);
}
line += "-tail";
console.log(log.last.length, line.length);
