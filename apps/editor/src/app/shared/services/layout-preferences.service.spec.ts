import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {DEFAULT_DRAWER_SETTINGS, drawerSettings} from '../components/drawer/drawer-settings';
import {LayoutPreferences} from '../models/config.model';
import {LAYOUT_PREFERENCES_STORAGE} from './layout-preferences-storage';
import {LayoutPreferencesService} from './layout-preferences.service';

describe('LayoutPreferencesService', () => {
    const load = vi.fn<() => Promise<unknown>>();
    const save = vi.fn<(preferences: LayoutPreferences) => Promise<void>>();
    let service: LayoutPreferencesService;

    beforeEach(() => {
        load.mockReset().mockResolvedValue(null);
        save.mockReset().mockResolvedValue(undefined);
        TestBed.configureTestingModule({
            providers: [{provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load, save}}],
        });
        service = TestBed.inject(LayoutPreferencesService);
    });

    afterEach(async () => {
        await service.flush().catch(() => undefined);
        vi.useRealTimers();
    });

    it('sets defaults on first initialization and loads the backend once', async () => {
        const [left, right] = await Promise.all([
            service.initialize('left', DEFAULT_DRAWER_SETTINGS, drawerSettings),
            service.initialize('right', DEFAULT_DRAWER_SETTINGS, drawerSettings),
        ]);
        expect(left).toEqual(DEFAULT_DRAWER_SETTINGS);
        expect(right).toEqual(DEFAULT_DRAWER_SETTINGS);
        expect(service.ready()).toBe(true);
        expect(load).toHaveBeenCalledTimes(1);
        expect(save).not.toHaveBeenCalled();
    });

    it('merges saved valid fields over defaults and preserves other components on save', async () => {
        load.mockResolvedValue({
            schema_version: 1,
            components: {
                left: {width: '40%', expanded: false, minHeight: -5},
                right: {width: '70%'},
            },
        });
        const config = await service.initialize('left', DEFAULT_DRAWER_SETTINGS, drawerSettings);
        expect(config).toEqual({...DEFAULT_DRAWER_SETTINGS, width: '40%', expanded: false});
        service.saveConfig('left', {...config, width: '45%'});
        await service.flush();
        expect(save).toHaveBeenLastCalledWith({
            schema_version: 1,
            components: {
                left: {...config, width: '45%'},
                right: {width: '70%'},
            },
        });
        expect(service.dirty()).toBe(false);
    });

    it.each(['unavailable', 'malformed'])(
        'falls back to defaults if preferences are %s',
        async (kind) => {
            if (kind === 'unavailable') load.mockRejectedValue(new Error('Backend unavailable'));
            else load.mockResolvedValue({schema_version: 2, components: {}});
            expect(await service.initialize('left', DEFAULT_DRAWER_SETTINGS, drawerSettings)).toEqual(
                DEFAULT_DRAWER_SETTINGS,
            );
            expect(service.error()).toBeTruthy();
            expect(save).not.toHaveBeenCalled();
        },
    );

    it('debounces changes across components into one snapshot', async () => {
        vi.useFakeTimers();
        await service.load();
        service.saveConfig('left', {width: '30%'});
        service.saveConfig('left', {width: '35%'});
        service.saveConfig('right', {expanded: false});
        await vi.advanceTimersByTimeAsync(299);
        expect(save).not.toHaveBeenCalled();
        await vi.advanceTimersByTimeAsync(1);
        expect(save).toHaveBeenCalledExactlyOnceWith({
            schema_version: 1,
            components: {
                left: {width: '35%'},
                right: {expanded: false},
            },
        });
    });

    it('serializes in-flight saves so a stale snapshot cannot overwrite newer edits', async () => {
        let finishFirst!: () => void;
        save.mockImplementationOnce(
            () =>
                new Promise<void>((resolve) => {
                    finishFirst = resolve;
                }),
        );
        await service.load();
        service.saveConfig('left', {width: '30%'});
        const first = service.flush();
        await vi.waitFor(() => expect(save).toHaveBeenCalledTimes(1));
        service.saveConfig('left', {width: '40%'});
        const second = service.flush();
        expect(save).toHaveBeenCalledTimes(1);
        finishFirst();
        await Promise.all([first, second]);
        expect(save).toHaveBeenCalledTimes(2);
        expect(save.mock.calls[1][0].components['left']).toEqual({width: '40%'});
        expect(service.dirty()).toBe(false);
    });

    it('retains unsaved state after failure and allows a retry', async () => {
        save.mockRejectedValueOnce(new Error('Disk full'));
        await service.load();
        service.saveConfig('left', {width: '30%'});
        await expect(service.flush()).rejects.toThrow('Disk full');
        expect(service.dirty()).toBe(true);
        expect(service.error()).toBe('Disk full');
        await service.flush();
        expect(service.dirty()).toBe(false);
        expect(service.error()).toBeNull();
        expect(save).toHaveBeenCalledTimes(2);
    });
});
