import {invoke} from '@tauri-apps/api/core';
import {listen, type UnlistenFn} from '@tauri-apps/api/event';

const windowCloseEvent = 'editor://window-close-requested';
let ownerUpdates = Promise.resolve();

function updateOwner(owner: string, enabled: boolean): Promise<void> {
    const next = ownerUpdates.then(() => invoke<void>('editor_set_window_close_review', {owner, enabled}));
    ownerUpdates = next.catch(() => undefined);
    return next;
}

export function watchDesktopClose(requestReview: () => void, reportFailure: () => void): () => void {
    const owner = crypto.getRandomValues(new Uint32Array(4)).join('-');
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    void listen<string>(windowCloseEvent, ({payload}) => {
        if (!disposed && payload === owner) requestReview();
    }, {target: {kind: 'WebviewWindow', label: 'main'}}).then(stop => {
        if (disposed) {
            stop();
            return;
        }
        unlisten = stop;
        return updateOwner(owner, true);
    }).catch(() => {
        unlisten?.();
        unlisten = undefined;
        if (!disposed) reportFailure();
    });

    return () => {
        disposed = true;
        if (!unlisten) return;
        unlisten();
        unlisten = undefined;
        void updateOwner(owner, false).catch(() => undefined);
    };
}
