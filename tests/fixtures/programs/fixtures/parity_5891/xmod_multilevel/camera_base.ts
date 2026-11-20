import { BaseObject, MatrixLike } from "./base_object.ts";
export class CameraBase extends BaseObject {
  declare matrixWorld: MatrixLike;
  declare matrixWorldInverse: MatrixLike;
  constructor() {
    super();
    (this as any).matrixWorldInverse = new MatrixLike("inverse");
    (this as any).cameraReady = true;
  }
}
