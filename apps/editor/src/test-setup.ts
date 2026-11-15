// Node 26 can reserve the global without providing usable browser storage.
const {jsdom} = globalThis as typeof globalThis & { jsdom?: { window: Window } };
if (typeof globalThis.localStorage === 'undefined' && jsdom) {
    Object.defineProperty(globalThis, 'localStorage', {
        configurable: true,
        value: jsdom.window.localStorage,
    });
}
