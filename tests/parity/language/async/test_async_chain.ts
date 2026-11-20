// Test chained async/await

async function step1(): Promise<number> {
    console.log(1);
    return 10;
}

async function step2(x: number): Promise<number> {
    console.log(2);
    return x + 20;
}

async function step3(x: number): Promise<number> {
    console.log(3);
    return x + 30;
}

async function runChain(): Promise<number> {
    let a = await step1();
    let b = await step2(a);
    let c = await step3(b);
    return c;
}

let result = await runChain();
console.log(result); // Should print 60 (10 + 20 + 30)
