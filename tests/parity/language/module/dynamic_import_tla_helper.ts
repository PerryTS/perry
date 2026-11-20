async function compute(): Promise<number> {
  return 99;
}

export const value: number = await compute();
export const tag: string = "tla-loaded";
