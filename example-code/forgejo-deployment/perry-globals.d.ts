// Minimal ambient declarations for Perry's built-in globals.
//
// Perry's runtime exposes `process` natively (env vars, exit, signal
// handlers — see crates/perry-runtime/src/process.rs); this file
// declares just enough of the surface for IDE typechecking. It is NOT
// `@types/node` — only the subset Perry actually implements.

declare const process: {
  env: Record<string, string | undefined>;
  exit(code?: number): never;
  on(
    event: 'SIGINT' | 'SIGTERM' | 'SIGHUP' | 'exit' | 'uncaughtException',
    handler: (...args: unknown[]) => void,
  ): void;
  argv: string[];
  cwd(): string;
  platform: string;
};
