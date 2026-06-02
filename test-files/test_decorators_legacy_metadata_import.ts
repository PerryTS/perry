import "reflect-metadata";
import { ModuleRepo, ModuleService } from "./decorator-metadata-module/service";

console.log("module injectable", Reflect.getMetadata("module:injectable", ModuleService));
console.log(
  "module paramtypes",
  Reflect.getMetadata("design:paramtypes", ModuleService)[0] === ModuleRepo,
);
