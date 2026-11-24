import {signal} from '@angular/core';
import {ComponentFixture, TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {By} from '@angular/platform-browser';
import {Splitter} from '../splitter/splitter';
import {LayoutPreferences} from '../../models/config.model';
import {
    LAYOUT_PREFERENCES_STORAGE, WORKSPACE_LAYOUT_SCOPE, type WorkspaceLayoutScope,
} from '../../services/layout-preferences-storage';
import {LayoutPreferencesService} from '../../services/layout-preferences.service';
import {DEFAULT_DRAWER_SETTINGS} from './drawer-settings';
import {Drawer} from './drawer';

describe('Drawer configuration', () => {
    const load = vi.fn<() => Promise<unknown>>();
    const save = vi.fn<(preferences: LayoutPreferences) => Promise<void>>();
    let fixture: ComponentFixture<Drawer>;
    let component: Drawer;
    let preferences: LayoutPreferencesService;
    const workspaceScope = signal<WorkspaceLayoutScope>(null);

    beforeEach(() => {
        load.mockReset().mockResolvedValue(null);
        save.mockReset().mockResolvedValue(undefined);
        workspaceScope.set(null);
        TestBed.configureTestingModule({
            imports: [Drawer],
            providers: [
                {provide: LAYOUT_PREFERENCES_STORAGE, useValue: {load, save}},
                {provide: WORKSPACE_LAYOUT_SCOPE, useValue: workspaceScope},
            ],
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

    it('switches to each project’s saved layout and keeps the legacy global value as a fallback', async () => {
        const first = 'a'.repeat(64);
        const second = 'b'.repeat(64);
        const key = 'editor.navigation';
        const legacy = {width: '44%'};
        workspaceScope.set(first);
        load.mockResolvedValue({
            schema_version: 1,
            components: {
                [key]: legacy,
                ['project:' + second + ':' + key]: {width: '70%'},
            },
        });
        await initialize();
        expect(component.config().width).toBe('44%');
        component.config.update(config => ({...config, width: '55%', collapsed: true}));
        fixture.detectChanges();

        workspaceScope.set(undefined);
        fixture.detectChanges();
        expect(component.configReady()).toBe(false);
        workspaceScope.set(second);
        await initialize();
        expect(component.config().width).toBe('70%');
        expect(component.config().collapsed).toBe(DEFAULT_DRAWER_SETTINGS.collapsed);
        component.config.update(config => ({...config, width: '75%'}));
        fixture.detectChanges();
        await preferences.flush();
        const written = save.mock.calls.at(-1)![0].components;
        expect(written[key]).toEqual(legacy);
        expect(written['project:' + first + ':' + key]?.['width']).toBe('55%');
        expect(written['project:' + second + ':' + key]?.['width']).toBe('75%');

        workspaceScope.set(first);
        await initialize();
        expect(component.config().width).toBe('55%');
        expect(component.config().collapsed).toBe(true);
    });

    it('shows legacy settings when project identity is unavailable without altering them', async () => {
        workspaceScope.set(false);
        load.mockResolvedValue({
            schema_version: 1,
            components: {'editor.navigation': {width: '37%'}},
        });
        await initialize();
        expect(component.config().width).toBe('37%');
        component.config.update(config => ({...config, width: '64%'}));
        fixture.detectChanges();
        await preferences.flush();
        expect(save).not.toHaveBeenCalled();
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

    it('clamps resizing to the configured limits and autosaves the new size', async () => {
        fixture.componentRef.setInput('config', {
            width: '240px',
            minWidth: '192px',
            maxWidth: '400px',
        });
        await initialize();
        const element: HTMLElement = fixture.nativeElement;
        vi.spyOn(element, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 240, 600));
        vi.spyOn(element.parentElement!, 'getBoundingClientRect').mockReturnValue(
            new DOMRect(0, 0, 1000, 600),
        );
        const splitter: Splitter = fixture.debugElement.query(By.directive(Splitter)).componentInstance;
        splitter.resize.emit(1000);
        fixture.detectChanges();
        expect(component.config().width).toBe('400px');
        await preferences.flush();
        expect(save.mock.calls.at(-1)?.[0].components['editor.navigation']['width']).toBe('400px');
        splitter.edge.emit('min');
        expect(component.config().width).toBe('192px');
    });

    it('resizes right drawers in the opposite direction', async () => {
        fixture.componentRef.setInput('type', 'right');
        fixture.componentRef.setInput('config', {
            width: '240px',
            minWidth: '192px',
            maxWidth: '400px',
        });
        await initialize();
        const element: HTMLElement = fixture.nativeElement;
        vi.spyOn(element, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 0, 240, 600));
        vi.spyOn(element.parentElement!, 'getBoundingClientRect').mockReturnValue(
            new DOMRect(0, 0, 1000, 600),
        );
        const splitter: Splitter = fixture.debugElement.query(By.directive(Splitter)).componentInstance;
        splitter.resize.emit(20);
        expect(component.config().width).toBe('220px');
    });

    it('uses the panel width limit in place of an older saved maximum', async () => {
        load.mockResolvedValue({
            schema_version: 1,
            components: {'editor.navigation': {width: '240px', maxWidth: '288px'}},
        });
        fixture.componentRef.setInput('maxWidth', '45%');
        await initialize();

        const element: HTMLElement = fixture.nativeElement;
        vi.spyOn(element.parentElement!, 'getBoundingClientRect').mockReturnValue(
            new DOMRect(0, 0, 1000, 600),
        );
        const splitter: Splitter = fixture.debugElement.query(By.directive(Splitter)).componentInstance;
        splitter.edge.emit('max');

        expect(component.config().width).toBe('450px');
    });
});
