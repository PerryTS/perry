type Off = () => string;
const listeners: (() => string)[] = [];
const ev = { on(cb: () => string): Off { listeners.push(cb); return () => "off-called"; } };

function main(): void {
  const fact = (n: number): number => (n <= 1 ? 1 : n * fact(n - 1));
  console.log("fact=" + fact(5));

  const off = ev.on(() => off());
  console.log("off=" + listeners[0]!());

  const sub = { unsub: (): string => "unsubbed" };
  const sub2 = ((o: { next: () => string }) => { listeners.push(o.next); return sub; })({ next: () => sub2.unsub() });
  console.log("sub2=" + listeners[1]!());

  const a = (): string => b() + "/a",
    b = (): string => "b";
  console.log("multi=" + a());

  const fib = function rec(n: number): number { return n < 2 ? n : rec(n - 1) + rec(n - 2); };
  console.log("fib=" + fib(10));

  // mutual recursion across statements
  const even = (n: number): boolean => (n === 0 ? true : odd(n - 1));
  const odd = (n: number): boolean => (n === 0 ? false : even(n - 1));
  console.log("even10=" + even(10) + " odd7=" + odd(7));
}
main();
