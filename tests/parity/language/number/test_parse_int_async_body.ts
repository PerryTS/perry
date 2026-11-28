// parseInt and parseFloat inside an async function body read their arguments as they
// do in a synchronous one: the radix selects the base, an omitted radix detects `0x`.
async function parse(s: string, r: number): Promise<void> {
  console.log(parseInt(s), parseInt("ff", 16), parseInt(s, r), parseInt("0x1F"), parseInt("ff", 37));
  console.log(parseFloat("3.5abc"), parseFloat("x"), parseFloat(" -0.25e2 "));
}
parse("42", 8);
