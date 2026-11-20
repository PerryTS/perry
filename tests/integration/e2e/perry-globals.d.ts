// Minimal ambient declarations for Perry's built-in globals (subset
// the e2e tests actually use).

declare const process: {
  env: Record<string, string | undefined>;
  exit(code?: number): never;
  argv: string[];
};
