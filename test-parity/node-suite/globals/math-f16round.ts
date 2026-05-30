function formatNumber(value: number): string {
  if (Object.is(value, -0)) return "-0";
  if (Number.isNaN(value)) return "NaN";
  return String(value);
}

function show(label: string, value: number): void {
  console.log(label, formatNumber(value));
}

function showSame(label: string, value: number, expected: number): void {
  console.log(label, Object.is(value, expected));
}

console.log("typeof:", typeof Math.f16round);
console.log("name:", Math.f16round.name);
console.log("length:", Math.f16round.length);

show("direct 1.337:", Math.f16round(1.337));
show("direct -0:", Math.f16round(-0));
show("direct NaN:", Math.f16round(NaN));
show("direct Infinity:", Math.f16round(Infinity));
show("direct -Infinity:", Math.f16round(-Infinity));
showSame("underflow tie:", Math.f16round(2.9802322387695312e-8), 0);
showSame(
  "subnormal tie up:",
  Math.f16round(8.940696716308594e-8),
  1.1920928955078125e-7,
);
showSame(
  "min subnormal:",
  Math.f16round(5.960464477539063e-8),
  5.960464477539063e-8,
);
showSame("min normal:", Math.f16round(0.00006103515625), 0.00006103515625);
show("max finite:", Math.f16round(65504));
show("overflow below tie:", Math.f16round(65519));
show("overflow tie:", Math.f16round(65520));
showSame(
  "negative underflow tie:",
  Math.f16round(-2.9802322387695312e-8),
  -0,
);
show("ties even down:", Math.f16round(1.00048828125));
show("ties even up:", Math.f16round(1.00146484375));

const alias = Math.f16round;
console.log("alias typeof:", typeof alias);
show("alias 1.337:", alias(1.337));

const globalAlias = globalThis.Math.f16round;
console.log("global alias typeof:", typeof globalAlias);
showSame(
  "global alias subnormal:",
  globalAlias(8.940696716308594e-8),
  1.1920928955078125e-7,
);
