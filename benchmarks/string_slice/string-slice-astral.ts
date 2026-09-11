function inspect(input: string): string {
  let s = input;
  let out = "";
  for (let i = 0; i < 16 && s.length; i++) {
    out += s.length + ":" + s.charCodeAt(0) + ",";
    s = s.slice(1);
  }
  return out;
}
console.log(inspect("ä中😀Ö"));
