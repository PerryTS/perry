// Issue #1788 (follow-up): a REST-param inherited static method through a
// class-expression-value parent. effect's `pipe(...args)` / `dual` are
// rest-param static methods called during Schema init; the inherited-static
// dispatcher must bundle the trailing call args into the rest array rather
// than dropping them.
//
// Expected output:
// pipe 3: piped:3:X
// pipe 0: piped:0:X
// mixed: a|2:M

function make(tag: string) {
  return class {
    static ast = tag;
    static pipe(...args: number[]) {
      return "piped:" + args.length + ":" + this.ast;
    }
    static mixed(first: string, ...rest: number[]) {
      return first + "|" + rest.length + ":" + this.ast;
    }
  };
}

class Sub extends make("X") {}
console.log("pipe 3:", (Sub as any).pipe(1, 2, 3));
console.log("pipe 0:", (Sub as any).pipe());

class Mid extends make("M") {}
class Leaf extends Mid {}
console.log("mixed:", (Leaf as any).mixed("a", 10, 20));
