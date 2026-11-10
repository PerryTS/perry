export class Object3D {
    public visible = true;

    constructor() {
        Object.defineProperties(this, {
            matrixWorld: {
                value: { source: "Object3D.matrixWorld", elements: 16 },
                writable: true,
                enumerable: true,
                configurable: true,
            },
            matrixWorldNeedsUpdate: {
                value: false,
                writable: true,
                enumerable: true,
                configurable: true,
            },
        });
        Object.defineProperty(this, "matrixAutoUpdate", {
            value: true,
            writable: true,
            enumerable: true,
            configurable: true,
        });
    }
}

export class Camera extends Object3D {
    constructor() {
        super();
        (this as any).isCamera = true;
    }
}

export class Texture {
    public textureField = "base-field";
    private _image: any;

    constructor(image: any = { data: "default", width: 1, height: 1 }) {
        this.image = image;
    }

    get image(): any {
        return this._image;
    }

    set image(value: any) {
        this._image = {
            data: value && value.data,
            width: value && value.width,
            height: value && value.height,
            assignedBy: "Texture.image",
        };
    }
}
