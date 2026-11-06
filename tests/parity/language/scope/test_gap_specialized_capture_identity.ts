function outer(label: string) {
  function build<T>(value: T) {
    let current = value;
    let caught = "";
    try { throw "ok"; } catch (error) { caught = String(error); }
    return (next: T = value) => {
      current = next;
      return () => label + ":" + String(current) + ":" + caught;
    };
  }
  return [build<number>(1), build<string>("x")];
}
const [number, text] = outer("captured");
const a = number(2);
const b = text("y");
console.log(a(), b(), number()(), text()());
