export interface EditorChangePage {
    readonly cursor: number;
    readonly resyncRequired: boolean;
    readonly revisions: readonly number[];
}

function record(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function cursor(value: unknown): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
}

export function editorReceiptCursor(update: unknown): number | null {
    if (!record(update) || !record(update['receipt'])) return null;
    const value = update['receipt']['change_cursor'];
    return cursor(value) ? value : null;
}

/** The journal carries invalidation metadata, so only authored changes need snapshot catch-up. */
export function readEditorChangePage(value: unknown, after: number): EditorChangePage {
    if (!record(value) || !cursor(value['cursor']) || value['cursor'] < after ||
        typeof value['resync_required'] !== 'boolean' || !Array.isArray(value['events']) ||
        value['events'].length > 256 || value['resync_required'] && value['events'].length !== 0) {
        throw new Error('The editor returned an invalid change page.');
    }
    const revisions: number[] = [];
    let previous = after;
    for (const entry of value['events'] as readonly unknown[]) {
        if (!record(entry) || !cursor(entry['sequence']) || entry['sequence'] !== previous + 1 ||
            entry['sequence'] > value['cursor'] || !cursor(entry['revision']) || !record(entry['event'])) {
            throw new Error('The editor returned out-of-order change events.');
        }
        previous = entry['sequence'];
        switch (entry['event']['kind']) {
            case 'document_changed':
            case 'project_changed':
            case 'state_changed':
                revisions.push(entry['revision']);
                break;
            case 'simulation_advanced':
            case 'runtime_scene_changed':
                break;
            default:
                throw new Error('The editor returned an unknown change event.');
        }
    }
    if (!value['resync_required'] && previous !== value['cursor']) {
        throw new Error('The editor change page is incomplete.');
    }
    return {cursor: value['cursor'], resyncRequired: value['resync_required'], revisions};
}
