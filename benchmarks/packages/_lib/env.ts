// Server endpoints for the I/O workloads. scripts/package_bench.py starts
// the servers (own data dirs, own ports) and exports these variables.
export function port(name: string): number {
  const raw = process.env[name];
  if (raw === undefined || raw === "") throw new Error(name + " is not set (start servers via scripts/package_bench.py)");
  return parseInt(raw, 10);
}
export const HOST = "127.0.0.1";
