enum Color { Red, Green, Blue }
enum Dir { Up = 1, Down }
enum S { A = "aaa", B = "bbb" }
const c: Color = Color.Blue;
if (Color[c] !== "Blue") throw new Error(`reverse dyn: ${Color[c]}`);
if (Color[0] !== "Red" || Color[1] !== "Green" || Color[2] !== "Blue") throw new Error("reverse literal");
if (Dir[1] !== "Up" || Dir[2] !== "Down") throw new Error("reverse explicit-base");
if ((Color["Blue"] as number) !== 2 || (Dir["Down"] as number) !== 2) throw new Error("forward computed");
if (Color.Blue !== 2 || Dir.Down !== 2) throw new Error("forward member");
if (S.A !== "aaa" || S["B"] !== "bbb") throw new Error("string forward");
if ((S as any)["aaa"] !== undefined) throw new Error("string reverse");
if ((Color as any)[99] !== undefined) throw new Error("out-of-range reverse");
console.log("numeric enum reverse mapping ok");
