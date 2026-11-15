export const HOST_A = "--host-a";
export const HOST_B = "--host-b";
export function isHost(a: string[]): boolean {
  return a[0] === HOST_A || a[0] === HOST_B;
}
