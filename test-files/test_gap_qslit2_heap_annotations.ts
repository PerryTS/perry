// A numeric annotation is erased: a movable string remains an ordinary
// NaN-boxed field, including while its completed literal survives collections.
function annotated(value: any) {
  return { f: () => 5, n: value as number };
}
let sum = 0;
for (let i = 0; i < 200; i++) {
  const text = "heap-" + i + "-" + (i * i);
  const saved = annotated(text);
  for (let j = 0; j < 12; j++) {
    const churn = annotated("churn-" + i + "-" + j);
    sum += String(churn.n).length + churn.f();
  }
  console.log(i === 199 ? String(saved.n) : "", typeof saved.n, saved.f());
  sum += String(saved.n).length;
}
console.log(sum);
