import {TestBed} from '@angular/core/testing';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {vi} from 'vitest';
import {LayoutPreferences} from '../models/config.model';
import {LAYOUT_PREFERENCES_STORAGE, LAYOUT_STORAGE_KEY} from './layout-preferences-storage';

vi.mock('@tauri-apps/api/core', () => ({isTauri: vi.fn(), invoke: vi.fn()}));

describe('Layout preferences storage', () => {
    const preferences: LayoutPreferences = {
        schema_version: 1,
        components: {
            'editor.navigation': {width: '35%', expanded: false},
        },
    };

    beforeEach(() => {
        vi.mocked(isTauri).mockReset().mockReturnValue(false);
        vi.mocked(invoke).mockReset();
        localStorage.removeItem(LAYOUT_STORAGE_KEY);
    });

    afterEach(() => localStorage.removeItem(LAYOUT_STORAGE_KEY));

    it('stores standalone preferences across service initialization', async () => {
        const storage = TestBed.inject(LAYOUT_PREFERENCES_STORAGE);
        expect(await storage.load()).toBeNull();
        await storage.save(preferences);
        TestBed.resetTestingModule();
        expect(await TestBed.inject(LAYOUT_PREFERENCES_STORAGE).load()).toEqual(preferences);
        expect(invoke).not.toHaveBeenCalled();
    });

    it('uses the native preferences commands in Tauri', async () => {
        vi.mocked(isTauri).mockReturnValue(true);
        vi.mocked(invoke).mockResolvedValueOnce(preferences).mockResolvedValueOnce(undefined);
        const storage = TestBed.inject(LAYOUT_PREFERENCES_STORAGE);
        expect(await storage.load()).toEqual(preferences);
        await storage.save(preferences);
        expect(invoke).toHaveBeenNthCalledWith(1, 'editor_layout_preferences_load');
        expect(invoke).toHaveBeenNthCalledWith(2, 'editor_layout_preferences_save', {preferences});
        expect(localStorage.getItem(LAYOUT_STORAGE_KEY)).toBeNull();
    });
});
