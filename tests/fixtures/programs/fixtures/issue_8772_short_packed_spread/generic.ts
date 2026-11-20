export function invoke(
  instance: any,
  entity: { id: number },
  args: number[],
): void {
  instance.reset(entity, ...args);
}
