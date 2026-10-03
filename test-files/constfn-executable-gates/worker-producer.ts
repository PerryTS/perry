export function make(value: number) { return { x: value, m: () => value }; }
export class WorkerMade {
    x = 4;
    m = () => this.x;
}
