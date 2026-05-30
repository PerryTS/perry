function show(label: string, fn: () => unknown) {
  try {
    console.log(label, fn());
  } catch (err: any) {
    console.log(label, err?.name, err?.code ?? "no-code");
  }
}

show("fixed omitted", () => (1.23).toFixed());
show("fixed undefined", () => (1.23).toFixed(undefined as any));
show("fixed fractional", () => (1.23).toFixed(2.9));
show("fixed negative", () => (1).toFixed(-1));
show("fixed over", () => (1).toFixed(101));
show("fixed infinity", () => (Infinity).toFixed(2));
show("fixed infinity over", () => (Infinity).toFixed(101));
show("fixed nan", () => (NaN).toFixed(2));
show("fixed large", () => (1e21).toFixed(2));

show("precision omitted", () => (1).toPrecision());
show("precision undefined", () => (1.23).toPrecision(undefined as any));
show("precision zero", () => (1).toPrecision(0));
show("precision negative", () => (1).toPrecision(-1));
show("precision over", () => (1).toPrecision(101));
show("precision fractional", () => (12345).toPrecision(3.9));
show("precision infinity over", () => (Infinity).toPrecision(101));
show("precision nan over", () => (NaN).toPrecision(101));

show("exponential omitted", () => (1.23).toExponential());
show("exponential undefined", () => (1.23).toExponential(undefined as any));
show("exponential negative", () => (1).toExponential(-1));
show("exponential over", () => (1).toExponential(101));
show("exponential fractional", () => (1.23).toExponential(2.9));
show("exponential infinity over", () => (Infinity).toExponential(101));
show("exponential nan over", () => (NaN).toExponential(101));
