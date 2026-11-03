export function escDeep(s: any): void { (globalThis as any).e = s.host; }
export function bindDeep(s: any): any { const host = s.host; return host.get2.bind(host); }
