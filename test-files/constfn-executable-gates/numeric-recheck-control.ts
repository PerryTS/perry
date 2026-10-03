// Keep descriptor syntax outside the arithmetic module so it remains a
// runtime invalidation, rather than suppressing region formation globally.
export function mutateNumericReceiver(receiver:any):void {
    Object.defineProperty(receiver, "a", {
        get: () => 3, enumerable: true, configurable: true,
    });
}
