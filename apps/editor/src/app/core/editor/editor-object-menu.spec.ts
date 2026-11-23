import {describe, expect, it} from 'vitest';
import {allowsFolder, availableObjectMenuActions, objectMenuEntry} from './editor-object-menu';

describe('Object catalog availability', () => {
    const primitives = ['create_plane', 'create_cube', 'create_sphere', 'create_capsule'] as const;

    it('shows every built-in shape together once the native host supports the catalog', () => {
        const unavailable = availableObjectMenuActions(new Set(['create_empty']), false);
        const available = availableObjectMenuActions(new Set(['create_empty']), true);
        expect(available.filter(id => !unavailable.includes(id))).toEqual(primitives);
        for (const id of primitives) {
            const entry = objectMenuEntry(id)!;
            expect(entry.action.kind).toBe('primitive');
            expect(allowsFolder(entry)).toBe(true);
        }
        expect(allowsFolder(objectMenuEntry('create_point_light')!)).toBe(false);
    });
});
