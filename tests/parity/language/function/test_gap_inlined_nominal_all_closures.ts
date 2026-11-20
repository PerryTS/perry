function nominal() {
  return Object.assign((input: any) => input, {
    option: (input: any) => ({ value: input }),
    result: (input: any) => ({ success: input }),
    is: (_: any) => true
  });
}
function all() { return nominal(); }
const a = nominal(), b = all();
console.log(a(1), b("b"), a.is(1), b.option("b").value);
