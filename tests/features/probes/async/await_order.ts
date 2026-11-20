async function run() {
    const order = ["start"];
    await Promise.resolve();
    order.push("after");
    return order.join(">");
}

run().then((value) => {
    console.log(`async:${value}`);
});
