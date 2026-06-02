import "reflect-metadata";

function Injectable() {
  return function (target: any) {
    Reflect.defineMetadata("injectable", true, target);
  };
}

function Inject() {
  return function (target: any, _propertyKey: any, parameterIndex: number) {
    Reflect.defineMetadata("param:" + parameterIndex, true, target);
  };
}

class Repo {}

@Injectable()
class Service {
  constructor(@Inject() repo: Repo) {}
}

console.log("injectable", Reflect.getMetadata("injectable", Service));
console.log("paramtypes", Reflect.getMetadata("design:paramtypes", Service)[0] === Repo);
console.log("param0", Reflect.hasMetadata("param:0", Service));
