import "reflect-metadata";

class Repo {}

function Inject(token?: any): PropertyDecorator & ParameterDecorator {
  return (target: object, key: string | symbol | undefined, index?: number) => {
    let type = token || Reflect.getMetadata("design:type", target, key!);
    if (index !== undefined) {
      const deps = Reflect.getMetadata("self:paramtypes", target) || [];
      Reflect.defineMetadata("self:paramtypes", [...deps, { index, param: type }], target);
      return;
    }

    const props = Reflect.getMetadata("self:properties_metadata", (target as any).constructor) || [];
    Reflect.defineMetadata(
      "self:properties_metadata",
      [...props, { key, type }],
      (target as any).constructor,
    );
  };
}

class Service {
  @Inject()
  repo!: Repo;

  constructor(@Inject("manual") repo: Repo) {}
}

const props = Reflect.getMetadata("self:properties_metadata", Service);
console.log("property dep", props[0].key, props[0].type === Repo);
console.log("design type", Reflect.getOwnMetadata("design:type", Service, "repo") === Repo);
console.log("has own", Reflect.hasOwnMetadata("design:type", Service, "repo"));
console.log("keys", Reflect.getMetadataKeys(Service, "repo").join("|"));

const deps = Reflect.getMetadata("self:paramtypes", Service);
console.log("ctor inject", deps[0].index, deps[0].param);

Reflect.defineMetadata("temp", true, Service);
console.log("delete", Reflect.deleteMetadata("temp", Service), Reflect.hasMetadata("temp", Service));
