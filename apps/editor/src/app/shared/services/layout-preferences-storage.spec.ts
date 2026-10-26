import {vi} from 'vitest';
import {LayoutPreferences} from '../models/config.model';
import {createLayoutPreferencesStorage, LAYOUT_STORAGE_KEY} from './layout-preferences-storage';

describe('Layout preferences storage', () => {
    const invoke = vi.fn<(operation: string, args?: Record<string, unknown>) => Promise<unknown>>();
    const preferences: LayoutPreferences = {
        schema_version: 1,
        components: {
            'editor.navigation': {width: '35%', expanded: false},
        },
    };

    beforeEach(() => {
        invoke.mockReset();
        localStorage.removeItem(LAYOUT_STORAGE_KEY);
    });

    afterEach(() => localStorage.removeItem(LAYOUT_STORAGE_KEY));

    it('stores standalone preferences across service initialization', async () => {
        const storage = createLayoutPreferencesStorage(false, invoke);
        expect(await storage.load()).toBeNull();
        await storage.save(preferences);
        expect(await createLayoutPreferencesStorage(false, invoke).load()).toEqual(preferences);
        expect(invoke).not.toHaveBeenCalled();
    });

    it('uses the native preferences commands in Tauri', async () => {
        invoke.mockResolvedValueOnce(preferences).mockResolvedValueOnce(undefined);
        const storage = createLayoutPreferencesStorage(true, invoke);
        expect(await storage.load()).toEqual(preferences);
        await storage.save(preferences);
        expect(invoke).toHaveBeenNthCalledWith(1, 'editor_layout_preferences_load');
        expect(invoke).toHaveBeenNthCalledWith(2, 'editor_layout_preferences_save', {preferences});
        expect(localStorage.getItem(LAYOUT_STORAGE_KEY)).toBeNull();
    });
});
