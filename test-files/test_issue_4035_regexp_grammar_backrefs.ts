function check(name: string, value: boolean) {
  if (!value) {
    throw new Error(name);
  }
  console.log(name + ": ok");
}

let syntaxOk = false;
try {
  new RegExp("a**");
} catch (e) {
  syntaxOk = e instanceof SyntaxError;
}
check("invalid-pattern-syntaxerror", syntaxOk);

const control = new RegExp("\\cA").exec(String.fromCharCode(1));
check("control-escape", control !== null && control[0] === String.fromCharCode(1));

const identity = new RegExp("\\<").exec("<");
check("identity-escape", identity !== null && identity[0] === "<");

const nul = /\0/.exec("\u0000");
check("nul-escape", nul !== null && nul[0] === "\u0000");

const simple = /(A)\1/.exec("AA");
check("simple-backref", simple !== null && simple[0] === "AA" && simple[1] === "A");

const forward = /\1(A)(B)\2/.exec("ABB");
check(
  "forward-backref",
  forward !== null && forward[0] === "ABB" && forward[1] === "A" && forward[2] === "B",
);

const multi = /(A)\1(B)\2/.exec("AABB");
check(
  "multi-backref",
  multi !== null && multi[0] === "AABB" && multi[1] === "A" && multi[2] === "B",
);

const materialized = /[a-c\d]+/.exec("\n\nabc324234\n");
check(
  "exec-metadata",
  materialized !== null &&
    materialized[0] === "abc324234" &&
    materialized.index === 2 &&
    materialized.input === "\n\nabc324234\n",
);
