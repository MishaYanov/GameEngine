import {DOCUMENT, inject, provideAppInitializer, Service, signal} from '@angular/core';

export const EDITOR_THEMES = [
    {id: 'workbench', label: 'Workbench'},
    {id: 'graphite', label: 'Graphite (Classic)'},
] as const;

export type EditorThemeId = typeof EDITOR_THEMES[number]['id'];

const THEME_STORAGE_KEY = 'game-engine.editor.theme';
const DEFAULT_THEME: EditorThemeId = 'workbench';

function isEditorTheme(value: string | null | undefined): value is EditorThemeId {
    return EDITOR_THEMES.some(({id}) => id === value);
}

function readTheme(window: Window | null): EditorThemeId {
    try {
        const saved = window?.localStorage.getItem(THEME_STORAGE_KEY);
        if (isEditorTheme(saved)) return saved;
    } catch {
        // A WebView or browser can disable storage.
    }
    return DEFAULT_THEME;
}

/** A local appearance preference shared by the editor and component workbench. */
@Service()
export class EditorThemeService {
    private readonly document = inject(DOCUMENT);
    private readonly current = signal(readTheme(this.document.defaultView));
    readonly theme = this.current.asReadonly();

    constructor() {
        this.document.documentElement.dataset['theme'] = this.current();
    }

    setTheme(theme: string): void {
        if (!isEditorTheme(theme) || theme === this.current()) return;
        this.current.set(theme);
        this.document.documentElement.dataset['theme'] = theme;
        try {
            this.document.defaultView?.localStorage.setItem(THEME_STORAGE_KEY, theme);
        } catch {
            // The selection still applies for this session.
        }
    }
}

export function provideEditorTheme() {
    return provideAppInitializer(() => {
        inject(EditorThemeService);
    });
}
