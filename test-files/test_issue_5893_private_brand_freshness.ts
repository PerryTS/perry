function check(label: string, condition: boolean): void {
    console.log(label + ": " + condition.toString());
}

function throwsTypeError(callback: () => void): boolean {
    try {
        callback();
        return false;
    } catch (error) {
        return error instanceof TypeError;
    }
}

function makeDeclarationInstance(): any {
    class C {
        #value = "test262";

        #method(): string {
            return this.#value;
        }

        get #getter(): string {
            return this.#value;
        }

        set #setter(value: string) {
            this.#value = value;
        }

        readMethod(other: C): string {
            return other.#method();
        }

        readGetter(other: C): string {
            return other.#getter;
        }

        writeSetter(other: C, value: string): void {
            other.#setter = value;
        }

        hasValue(other: any): boolean {
            return #value in other;
        }
    }

    return new C();
}

function makeExpressionInstance(): any {
    const C = class {
        #value = "test262";

        #method(): string {
            return this.#value;
        }

        get #getter(): string {
            return this.#value;
        }

        set #setter(value: string) {
            this.#value = value;
        }

        readMethod(other: any): string {
            return other.#method();
        }

        readGetter(other: any): string {
            return other.#getter;
        }

        writeSetter(other: any, value: string): void {
            other.#setter = value;
        }

        hasValue(other: any): boolean {
            return #value in other;
        }
    };

    return new C();
}

function checkFreshBrands(label: string, make: () => any): void {
    const first = make();
    const second = make();

    check(label + " own method", first.readMethod(first) === "test262");
    check(label + " own getter", second.readGetter(second) === "test262");
    first.writeSetter(first, "changed");
    check(label + " own setter", first.readGetter(first) === "changed");
    check(label + " own in", first.hasValue(first));
    check(label + " cross-evaluation in", !first.hasValue(second));

    check(
        label + " cross-evaluation method",
        throwsTypeError(() => first.readMethod(second))
    );
    check(
        label + " cross-evaluation getter",
        throwsTypeError(() => first.readGetter(second))
    );
    check(
        label + " cross-evaluation setter",
        throwsTypeError(() => first.writeSetter(second, "wrong"))
    );
}

checkFreshBrands("declaration", makeDeclarationInstance);
checkFreshBrands("expression", makeExpressionInstance);

function makeStaticClass(): any {
    return class {
        static #value = "test262";
        static _written = "";

        static #method(): string {
            return this.#value;
        }

        static get #getter(): string {
            return this.#value;
        }

        static set #setter(value: string) {
            this._written = value;
        }

        static accessMethod(): string {
            return this.#method();
        }

        static accessGetter(): string {
            return this.#getter;
        }

        static accessSetter(value: string): void {
            this.#setter = value;
        }

        static hasValue(other: any): boolean {
            return #value in other;
        }
    };
}

function makeStaticDeclarationClass(): any {
    class StaticDeclarationC {
        static #value = "test262";
        static _written = "";

        static #method(): string {
            return this.#value;
        }

        static get #getter(): string {
            return this.#value;
        }

        static set #setter(value: string) {
            this._written = value;
        }

        static accessMethod(): string {
            return this.#method();
        }

        static accessGetter(): string {
            return this.#getter;
        }

        static accessSetter(value: string): void {
            this.#setter = value;
        }

        static hasValue(other: any): boolean {
            return #value in other;
        }
    }

    return StaticDeclarationC;
}

function checkFreshStaticBrands(label: string, make: () => any): void {
    const first = make();
    const second = make();
    check(label + " own method", first.accessMethod() === "test262");
    check(label + " own getter", second.accessGetter() === "test262");
    first.accessSetter("changed");
    check(label + " own setter", first._written === "changed");
    check(label + " own in", first.hasValue(first));
    check(label + " cross-evaluation in", !first.hasValue(second));
    check(
        label + " cross-evaluation method",
        throwsTypeError(() => first.accessMethod.call(second))
    );
    check(
        label + " cross-evaluation getter",
        throwsTypeError(() => first.accessGetter.call(second))
    );
    check(
        label + " own getter after throw",
        first.accessGetter() === "test262"
    );
    check(
        label + " cross-evaluation setter",
        throwsTypeError(() => first.accessSetter.call(second, "wrong"))
    );
}

checkFreshStaticBrands("static expression", makeStaticClass);
checkFreshStaticBrands("static declaration", makeStaticDeclarationClass);
