type DecoderName = "decodeURI" | "decodeURIComponent";

function runDecode(name: DecoderName, input: string): string {
  if (name === "decodeURI") return decodeURI(input);
  return decodeURIComponent(input);
}

function showDecode(name: DecoderName, input: string): void {
  try {
    console.log(name, input, "ok", JSON.stringify(runDecode(name, input)));
  } catch (err) {
    const e = err as Error;
    console.log(name, input, "throw", e.name, e.message, err instanceof URIError);
  }
}

for (const input of [
  "%",
  "%ZZ",
  "%E0%A4%A",
  "%C0%AF",
  "%ED%A0%80",
  "%80",
  "%F4%90%80%80",
]) {
  showDecode("decodeURI", input);
  showDecode("decodeURIComponent", input);
}

for (const input of [
  "%3B",
  "%2F",
  "%3F",
  "%3f",
  "%3A",
  "%40",
  "%26",
  "%3D",
  "%2B",
  "%24",
  "%2C",
  "%23",
]) {
  console.log("reserved", input, decodeURI(input), decodeURIComponent(input));
}

console.log("space", decodeURI("%20") === " ", decodeURIComponent("%20") === " ");
console.log(
  "utf8",
  decodeURI("%C3%A9") === "\u00e9",
  decodeURIComponent("%C3%A9") === "\u00e9",
);
console.log("percent", decodeURI("%25") === "%", decodeURIComponent("%25") === "%");
