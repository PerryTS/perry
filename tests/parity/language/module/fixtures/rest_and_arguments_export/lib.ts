export function fetch<T>(receiver: Map<string, T>, key: string): T;
export function fetch<T>(receiver: Map<string, T>, key: string, dflt: T): T;
export function fetch(receiver: unknown, key: unknown, arg?: unknown, ...more: unknown[]): unknown {
  const given = arguments.length;
  const rest = given === 2 ? [] : [arg, ...more];
  return `given=${given} more=${Array.isArray(more)} rest=${rest.length}`;
}
