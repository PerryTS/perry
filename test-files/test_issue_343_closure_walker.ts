type Channel = 'a' | 'b' | 'c';
const ALL: Channel[] = ['a', 'b', 'c'];
interface P { name?: string; urls?: string[]; queries?: string[]; braveQuery?: string }
interface Out { name: string; urls: string[]; queries: string[]; braveQuery?: string }

function load(raw: any): { products: Out[]; channels: Record<Channel, boolean> } {
  const products: Out[] = raw.products.map((p: P | null, i: number) => {
    if (!p || typeof p.name !== 'string') {
      throw new Error('product[' + i + "] missing 'name'");
    }
    return {
      name: p.name,
      urls: Array.isArray(p.urls) ? p.urls : [],
      queries: Array.isArray(p.queries) ? p.queries : [],
      braveQuery: typeof p.braveQuery === 'string' ? p.braveQuery : undefined,
    };
  });
  const enabled = (raw.channels ?? {}) as Partial<Record<Channel, boolean>>;
  const channels = Object.fromEntries(
    ALL.map((c) => [c, enabled[c] !== false]),
  ) as Record<Channel, boolean>;
  return { products, channels };
}

const r = load({ products: [{ name: 'foo' }], channels: { a: true } });
console.log('len=' + r.products.length);
console.log('channels.a=' + r.channels.a);
console.log('channels.b=' + r.channels.b);
