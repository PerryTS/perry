export const lazy = <T>(getter: () => T): (() => T) => () => getter();
export const ready = lazy(() => 1);
