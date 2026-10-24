import {ComponentFixture, TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {LayoutPreferences} from '../../models/config.model';
import {LAYOUT_PREFERENCES_STORAGE} from '../../services/layout-preferences-storage';
import {LayoutPreferencesService} from '../../services/layout-preferences.service';
import {DEFAULT_DRAWER_SETTINGS} from './drawer-settings';
import {Drawer} from './drawer';

describe('Drawer configuration', () => {
    const load = vi.fn<() => Promise<unknown>>();
    const save = vi.fn<(preferences: LayoutPreferences) => Promise<void>>();
    let fixture: ComponentFixture<Drawer>;
    let component: Drawer;
    let preferences: LayoutPreferencesService;

    beforeEach(() => {
        load.mockReset().mockResolvedValue(null);
        save.mockReset().mockResolvedValue(undefined);
        TestBed.configureTestingModule({
            imports: [Drawer],
            providers: [{provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load, save}}],
        });
        fixture = TestBed.createComponent(Drawer);
        component = fixture.componentInstance;
        preferences = TestBed.inject(LayoutPreferencesService);
        fixture.componentRef.setInput('preferenceKey', 'editor.navigation');
    });

    async function initialize() {
        fixture.detectChanges();
        await vi.waitFor(() => expect(component.configReady()).toBe(true));
        fixture.detectChanges();
    }

    it('initializes defaults including defaults provided by its parent', async () => {
        fixture.componentRef.setInput('config', {minWidth: '40px'});
        await initialize();
        expect(component.config()).toEqual({...DEFAULT_DRAWER_SETTINGS, minWidth: '40px'});
        expect(save).not.toHaveBeenCalled();
    });

    it('initializes saved settings and autosaves later signal edits', async () => {
        load.mockResolvedValue({
            schema_version: 1,
            components: {
                'editor.navigation': {width: '40%', expanded: false},
            },
        });
        await initialize();
        expect(component.config()).toEqual({
            ...DEFAULT_DRAWER_SETTINGS,
            width: '40%',
            expanded: false,
        });
        expect(component.isExpanded()).toBe(false);
        expect(save).not.toHaveBeenCalled();
        component.config.update((config) => ({...config, width: '50%'}));
        fixture.detectChanges();
        await preferences.flush();
        expect(save).toHaveBeenCalledExactlyOnceWith({
            schema_version: 1,
            components: {
                'editor.navigation': {...DEFAULT_DRAWER_SETTINGS, width: '50%', expanded: false},
            },
        });
    });

    it('preserves edits made while the backend is loading', async () => {
        let finishLoad!: (value: unknown) => void;
        load.mockImplementation(
            () =>
                new Promise((resolve) => {
                    finishLoad = resolve;
                }),
        );
        fixture.detectChanges();
        await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));
        component.config.update((config) => ({...config, width: '55%'}));
        finishLoad({
            schema_version: 1,
            components: {'editor.navigation': {width: '40%', expanded: false}},
        });
        await initialize();
        expect(component.config()).toEqual({
            ...DEFAULT_DRAWER_SETTINGS,
            width: '55%',
            expanded: false,
        });
        await preferences.flush();
        expect(save.mock.calls[0][0].components['editor.navigation']['width']).toBe('55%');
    });

    it('flushes the most recent edit when removed, before its save effect runs', async () => {
        await initialize();
        component.config.update((config) => ({...config, expanded: false}));
        fixture.destroy();
        await preferences.flush();
        expect(save.mock.calls[0][0].components['editor.navigation']['expanded']).toBe(false);
    });

    it('ignores a load response after component destruction', async () => {
        let finishLoad!: (value: unknown) => void;
        load.mockImplementation(
            () =>
                new Promise((resolve) => {
                    finishLoad = resolve;
                }),
        );
        fixture.detectChanges();
        await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(1));
        fixture.destroy();
        finishLoad({schema_version: 1, components: {'editor.navigation': {width: '80%'}}});
        await preferences.load();
        expect(component.configReady()).toBe(false);
        expect(component.config()).toEqual(DEFAULT_DRAWER_SETTINGS);
        expect(save).not.toHaveBeenCalled();
    });
});
