export function sha256(s: string): string {
  return "user-sha256(" + s + ")";
}
export function md5(s: string): string {
  return "user-md5(" + s + ")";
}
export function join(...parts: string[]): string {
  return "user-join(" + parts.join("|") + ")";
}
export function basename(p: string): string {
  return "user-basename(" + p + ")";
}
export function hostname(): string {
  return "user-hostname";
}
export function platform(): string {
  return "user-platform";
}
