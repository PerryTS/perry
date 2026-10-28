// Helper module for test_issue_636_namespace_call.ts (#636).
export const make = (s: string) => s.toUpperCase();
export function decl(n: number): number {
    return n * 2;
}
