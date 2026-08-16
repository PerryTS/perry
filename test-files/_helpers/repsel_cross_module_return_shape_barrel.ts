// Exercise origin-path and origin-name resolution together: the consumer uses
// a third spelling, while the return-shape proof belongs to the leaf export.
export {
  makeCrossModuleRow as makeBarrelRow,
} from "./repsel_cross_module_return_shape.ts";
