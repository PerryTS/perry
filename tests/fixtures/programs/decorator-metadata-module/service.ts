function Injectable() {
  return function (target: any) {
    Reflect.defineMetadata("module:injectable", true, target);
  };
}

export class ModuleRepo {}

@Injectable()
export class ModuleService {
  constructor(repo: ModuleRepo) {}
}
