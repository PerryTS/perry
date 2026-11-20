import { CameraBase } from "./camera_base.ts";
import { MatrixLike } from "./base_object.ts";
export class PerspectiveLike extends CameraBase {
  constructor(fov: number, aspect: number, near: number, far: number) {
    super();
    (this as any).isPerspectiveLike = true;
    (this as any).projectionMatrix = new MatrixLike(`projection:${fov}:${aspect}:${near}:${far}`);
  }
}
