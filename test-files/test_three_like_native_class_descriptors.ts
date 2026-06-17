import { Camera, Texture } from "./_helpers/three_like_descriptor_bases";

function assert(condition: any, label: string): void {
    if (!condition) {
        throw new Error(label);
    }
}

class PerspectiveCamera extends Camera {
    public fov: number;
    public aspect: number;
    public near: number;
    public far: number;

    constructor(fov = 50, aspect = 1, near = 0.1, far = 2000) {
        super();
        this.fov = fov;
        this.aspect = aspect;
        this.near = near;
        this.far = far;
        (this as any).isPerspectiveCamera = true;
    }
}

class DataTexture extends Texture {
    public isDataTexture = true;

    constructor(data: any = null, width = 1, height = 1) {
        super();
        this.image = { data, width, height };
    }
}

const camera = new PerspectiveCamera();
assert((camera as any).matrixWorld.source === "Object3D.matrixWorld", "inherited constructor defineProperties state");
assert((camera as any).matrixWorld.elements === 16, "descriptor value object survived");
assert((camera as any).matrixWorldNeedsUpdate === false, "defineProperties boolean state");
assert((camera as any).matrixAutoUpdate === true, "defineProperty state");
assert((camera as any).visible === true, "imported class field initializer");
assert((camera as any).isCamera === true, "imported superclass constructor body");
assert((camera as any).isPerspectiveCamera === true, "subclass constructor body");
assert(camera.fov === 50 && camera.aspect === 1 && camera.near === 0.1 && camera.far === 2000, "subclass default args");

const cameraKeys = Object.keys(camera as any);
assert(cameraKeys.includes("matrixWorld"), "defineProperties enumerable key");
assert(cameraKeys.includes("matrixAutoUpdate"), "defineProperty enumerable key");

const explicitTexture = new DataTexture("pixels", 4, 2);
assert((explicitTexture as any)._image !== undefined, "subclass write invoked inherited setter");
const explicitImage = explicitTexture.image;
assert(explicitImage !== undefined, "subclass read invoked inherited getter");
assert(explicitImage.data === "pixels", "subclass write reached inherited setter data");
assert(explicitImage.width === 4, "subclass write reached inherited setter width");
assert(explicitImage.height === 2, "subclass write reached inherited setter height");
assert(explicitImage.assignedBy === "Texture.image", "inherited setter dispatch");
assert(!(Object.keys(explicitTexture as any).includes("image")), "subclass setter write did not create own data image");
assert(Object.keys(explicitTexture as any).includes("_image"), "setter created backing image slot");
assert((explicitTexture as any).textureField === "base-field", "imported base class field");
assert(explicitTexture.isDataTexture === true, "subclass field initializer after super");

const defaultTexture = new DataTexture();
const defaultImage = defaultTexture.image;
assert(defaultImage !== undefined, "default subclass read invoked inherited getter");
assert(defaultImage.data === null, "default data propagated through subclass");
assert(defaultImage.width === 1, "default width propagated through subclass");
assert(defaultImage.height === 1, "default height propagated through subclass");

console.log("OK");
