import {TestBed} from '@angular/core/testing';
import {vi} from 'vitest';
import {EditorThemeService, provideEditorTheme} from './editor-theme.service';

const storageKey = 'game-engine.editor.theme';

describe('Editor appearance preference', () => {
    const root = document.documentElement;
    const window = document.defaultView!;

    afterEach(() => {
        vi.restoreAllMocks();
        window.localStorage.removeItem(storageKey);
        delete root.dataset['theme'];
    });

    it('applies a saved theme at startup and keeps later choices across app instances', () => {
        window.localStorage.setItem(storageKey, 'graphite');
        TestBed.configureTestingModule({providers: [provideEditorTheme()]});
        const themes = TestBed.inject(EditorThemeService);
        expect(root.dataset['theme']).toBe('graphite');

        themes.setTheme('workbench');
        TestBed.resetTestingModule();

        expect(TestBed.inject(EditorThemeService).theme()).toBe('workbench');
        expect(root.dataset['theme']).toBe('workbench');
        expect(window.localStorage.getItem(storageKey)).toBe('workbench');
    });

    it('falls back to Workbench when a saved theme is unknown and ignores invalid choices', () => {
        window.localStorage.setItem(storageKey, 'removed-theme');
        const themes = TestBed.inject(EditorThemeService);
        themes.setTheme('removed-theme');
        expect(themes.theme()).toBe('workbench');
        expect(root.dataset['theme']).toBe('workbench');
    });

    it('applies a theme for the session when storage is unavailable', () => {
        vi.spyOn(window, 'localStorage', 'get').mockImplementation(() => {
            throw new DOMException('Storage is unavailable', 'SecurityError');
        });
        const themes = TestBed.inject(EditorThemeService);
        expect(themes.theme()).toBe('workbench');
        themes.setTheme('graphite');
        expect(root.dataset['theme']).toBe('graphite');
    });
});
