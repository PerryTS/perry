function runEncode(name, input) {
  if (name === "encodeURI") return encodeURI(input);
  return encodeURIComponent(input);
}

function showEncode(label, name, input) {
  try {
    console.log(label, name, "ok", runEncode(name, input));
  } catch (err) {
    const e = err;
    console.log(label, name, "throw", e.name, e.message, err instanceof URIError);
  }
}

for (const [label, input] of [
  ["high-start", "\uD800"],
  ["high-end", "\uDBFF"],
  ["low-start", "\uDC00"],
  ["low-end", "\uDFFF"],
  ["mixed-high", "a\uD800b"],
  ["mixed-low", "a\uDC00b"],
]) {
  showEncode(label, "encodeURI", input);
  showEncode(label, "encodeURIComponent", input);
}

console.log("pair uri", encodeURI("\uD83D\uDE00"));
console.log("pair component", encodeURIComponent("\uD83D\uDE00"));
console.log("reserved uri", encodeURI(";/?:@&=+$,# alpha"));
console.log("reserved component", encodeURIComponent(";/?:@&=+$,# alpha"));
console.log("bmp", encodeURI("caf\u00e9"), encodeURIComponent("caf\u00e9"));
