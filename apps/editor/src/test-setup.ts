// Use the storage for this jsdom window even when Node also exposes localStorage.
const {jsdom} = globalThis as typeof globalThis & { jsdom?: { window: Window } };
if (jsdom) {
    Object.defineProperty(globalThis, 'localStorage', {
        configurable: true,
        value: jsdom.window.localStorage,
    });
}
