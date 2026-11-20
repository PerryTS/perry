// Complex real-world patterns that combine multiple features and stress
// the compiler: data structures, algorithms, design patterns, state machines
// These are the kinds of patterns that expose compiler bugs at the intersection
// of features (closures + generics, classes + arrays + methods, etc.)

// === Pattern 1: Linked list with generic type ===
class ListNode<T> {
    value: T;
    next: ListNode<T> | null;

    constructor(value: T) {
        this.value = value;
        this.next = null;
    }
}

function listToArray<T>(head: ListNode<T> | null): T[] {
    const result: T[] = [];
    let current = head;
    while (current !== null) {
        result.push(current.value);
        current = current.next;
    }
    return result;
}

const n1 = new ListNode<number>(1);
const n2 = new ListNode<number>(2);
const n3 = new ListNode<number>(3);
n1.next = n2;
n2.next = n3;
console.log(listToArray(n1).join(","));  // 1,2,3

// === Pattern 2: Event system with typed callbacks ===
class TypedEventEmitter {
    private handlers: Map<string, Array<(data: any) => void>>;

    constructor() {
        this.handlers = new Map();
    }

    on(event: string, handler: (data: any) => void): void {
        if (!this.handlers.has(event)) {
            this.handlers.set(event, []);
        }
        this.handlers.get(event)!.push(handler);
    }

    emit(event: string, data: any): void {
        const handlers = this.handlers.get(event);
        if (handlers) {
            for (let i = 0; i < handlers.length; i++) {
                handlers[i](data);
            }
        }
    }
}

const emitter = new TypedEventEmitter();
const log: string[] = [];
emitter.on("click", (data: any) => log.push("click:" + data));
emitter.on("hover", (data: any) => log.push("hover:" + data));
emitter.on("click", (data: any) => log.push("click2:" + data));
emitter.emit("click", "btn1");
emitter.emit("hover", "div1");
console.log(log.join(","));  // click:btn1,click2:btn1,hover:div1

// === Pattern 3: State machine ===
class StateMachine {
    private state: string;
    private transitions: Record<string, Record<string, string>>;

    constructor(initial: string) {
        this.state = initial;
        this.transitions = {};
    }

    addTransition(from: string, event: string, to: string): void {
        if (!this.transitions[from]) {
            this.transitions[from] = {};
        }
        this.transitions[from][event] = to;
    }

    send(event: string): boolean {
        const fromState = this.transitions[this.state];
        if (fromState && fromState[event]) {
            this.state = fromState[event];
            return true;
        }
        return false;
    }

    getState(): string {
        return this.state;
    }
}

const sm = new StateMachine("idle");
sm.addTransition("idle", "start", "running");
sm.addTransition("running", "pause", "paused");
sm.addTransition("paused", "resume", "running");
sm.addTransition("running", "stop", "idle");

console.log(sm.getState());       // idle
console.log(sm.send("start"));    // true
console.log(sm.getState());       // running
console.log(sm.send("pause"));    // true
console.log(sm.getState());       // paused
console.log(sm.send("resume"));   // true
console.log(sm.getState());       // running
console.log(sm.send("stop"));     // true
console.log(sm.getState());       // idle
console.log(sm.send("pause"));    // false (invalid transition)
console.log(sm.getState());       // idle

// === Pattern 4: Pipeline / chain of transformations ===
class Pipeline<T> {
    private steps: Array<(input: T) => T>;

    constructor() {
        this.steps = [];
    }

    pipe(fn: (input: T) => T): Pipeline<T> {
        this.steps.push(fn);
        return this;
    }

    execute(input: T): T {
        let result = input;
        for (let i = 0; i < this.steps.length; i++) {
            result = this.steps[i](result);
        }
        return result;
    }
}

const numPipeline = new Pipeline<number>()
    .pipe((x: number) => x * 2)
    .pipe((x: number) => x + 10)
    .pipe((x: number) => x / 3);

console.log(numPipeline.execute(5));   // (5*2+10)/3 = 6.666666666666667
console.log(numPipeline.execute(10));  // (10*2+10)/3 = 10

const strPipeline = new Pipeline<string>()
    .pipe((s: string) => s.trim())
    .pipe((s: string) => s.toUpperCase())
    .pipe((s: string) => "<<" + s + ">>");

console.log(strPipeline.execute("  hello  "));  // <<HELLO>>

// === Pattern 5: Observer pattern ===
class Observable<T> {
    private _value: T;
    private observers: Array<(newVal: T, oldVal: T) => void>;

    constructor(initial: T) {
        this._value = initial;
        this.observers = [];
    }

    get value(): T {
        return this._value;
    }

    set(newValue: T): void {
        const old = this._value;
        this._value = newValue;
        for (let i = 0; i < this.observers.length; i++) {
            this.observers[i](newValue, old);
        }
    }

    subscribe(fn: (newVal: T, oldVal: T) => void): void {
        this.observers.push(fn);
    }
}

const counter = new Observable<number>(0);
const changes: string[] = [];
counter.subscribe((newVal: number, oldVal: number) => {
    changes.push(oldVal.toString() + "->" + newVal.toString());
});

counter.set(1);
counter.set(5);
counter.set(3);
console.log(changes.join(","));  // 0->1,1->5,5->3
console.log(counter.value);      // 3

// === Pattern 6: Matrix operations ===
function createMatrix(rows: number, cols: number, fill: number = 0): number[][] {
    const m: number[][] = [];
    for (let i = 0; i < rows; i++) {
        const row: number[] = [];
        for (let j = 0; j < cols; j++) {
            row.push(fill);
        }
        m.push(row);
    }
    return m;
}

function matrixMultiply(a: number[][], b: number[][]): number[][] {
    const rows = a.length;
    const cols = b[0].length;
    const inner = b.length;
    const result = createMatrix(rows, cols);

    for (let i = 0; i < rows; i++) {
        for (let j = 0; j < cols; j++) {
            let sum = 0;
            for (let k = 0; k < inner; k++) {
                sum = sum + a[i][k] * b[k][j];
            }
            result[i][j] = sum;
        }
    }

    return result;
}

const matA = [[1, 2], [3, 4]];
const matB = [[5, 6], [7, 8]];
const product = matrixMultiply(matA, matB);
console.log(product[0].join(","));  // 19,22
console.log(product[1].join(","));  // 43,50

// === Pattern 7: Binary search ===
function binarySearch(arr: number[], target: number): number {
    let lo = 0;
    let hi = arr.length - 1;

    while (lo <= hi) {
        const mid = Math.floor((lo + hi) / 2);
        if (arr[mid] === target) return mid;
        if (arr[mid] < target) lo = mid + 1;
        else hi = mid - 1;
    }

    return -1;
}

const sorted = [2, 5, 8, 12, 16, 23, 38, 56, 72, 91];
console.log(binarySearch(sorted, 23));  // 5
console.log(binarySearch(sorted, 2));   // 0
console.log(binarySearch(sorted, 91));  // 9
console.log(binarySearch(sorted, 50));  // -1

// === Pattern 8: Recursive tree structure ===
class TreeNode {
    value: number;
    children: TreeNode[];

    constructor(value: number) {
        this.value = value;
        this.children = [];
    }

    addChild(child: TreeNode): void {
        this.children.push(child);
    }

    sum(): number {
        let total = this.value;
        for (let i = 0; i < this.children.length; i++) {
            total = total + this.children[i].sum();
        }
        return total;
    }

    depth(): number {
        if (this.children.length === 0) return 1;
        let maxChildDepth = 0;
        for (let i = 0; i < this.children.length; i++) {
            const d = this.children[i].depth();
            if (d > maxChildDepth) maxChildDepth = d;
        }
        return 1 + maxChildDepth;
    }
}

const root = new TreeNode(1);
const child1 = new TreeNode(2);
const child2 = new TreeNode(3);
const grandchild = new TreeNode(4);
root.addChild(child1);
root.addChild(child2);
child1.addChild(grandchild);

console.log(root.sum());    // 10
console.log(root.depth());  // 3

// === Pattern 9: String parser (tokenizer) ===
interface Token {
    type: string;
    value: string;
}

function tokenize(input: string): Token[] {
    const tokens: Token[] = [];
    let pos = 0;

    while (pos < input.length) {
        const ch = input[pos];

        if (ch === " " || ch === "\t") {
            pos++;
            continue;
        }

        if (ch >= "0" && ch <= "9") {
            let num = "";
            while (pos < input.length && input[pos] >= "0" && input[pos] <= "9") {
                num = num + input[pos];
                pos++;
            }
            tokens.push({ type: "number", value: num });
            continue;
        }

        if (ch === "+" || ch === "-" || ch === "*" || ch === "/") {
            tokens.push({ type: "operator", value: ch });
            pos++;
            continue;
        }

        if (ch === "(" || ch === ")") {
            tokens.push({ type: "paren", value: ch });
            pos++;
            continue;
        }

        pos++;
    }

    return tokens;
}

const tokens = tokenize("(1 + 2) * 3");
const tokenStrs = tokens.map((t: Token) => t.type + ":" + t.value);
console.log(tokenStrs.join(","));
// paren:(,number:1,operator:+,number:2,paren:),operator:*,number:3

// === Pattern 10: Builder with validation ===
class QueryBuilder {
    private table: string;
    private conditions: string[];
    private orderField: string;
    private limitCount: number;

    constructor(table: string) {
        this.table = table;
        this.conditions = [];
        this.orderField = "";
        this.limitCount = -1;
    }

    where(condition: string): QueryBuilder {
        this.conditions.push(condition);
        return this;
    }

    orderBy(field: string): QueryBuilder {
        this.orderField = field;
        return this;
    }

    limit(n: number): QueryBuilder {
        this.limitCount = n;
        return this;
    }

    build(): string {
        let query = "SELECT * FROM " + this.table;
        if (this.conditions.length > 0) {
            query = query + " WHERE " + this.conditions.join(" AND ");
        }
        if (this.orderField !== "") {
            query = query + " ORDER BY " + this.orderField;
        }
        if (this.limitCount >= 0) {
            query = query + " LIMIT " + this.limitCount.toString();
        }
        return query;
    }
}

const query = new QueryBuilder("users")
    .where("age > 18")
    .where("active = true")
    .orderBy("name")
    .limit(10)
    .build();

console.log(query);
// SELECT * FROM users WHERE age > 18 AND active = true ORDER BY name LIMIT 10
