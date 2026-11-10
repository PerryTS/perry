function message() {
    try {
        throw new Error("boom");
    } catch (error) {
        return (error as Error).message;
    } finally {
        // Exercise finally without changing the deterministic value.
    }
}

console.log(`error:${message()}`);
