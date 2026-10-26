import {Component, computed, input} from '@angular/core';

export const EDITOR_ICONS = {
    cube: 'm12 3 8 4.5v9L12 21l-8-4.5v-9L12 3Zm0 9L4 7.5m8 4.5 8-4.5M12 12v9',
    folder: 'M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z',
    layers: 'm12 3 9 5-9 5-9-5 9-5ZM3 12l9 5 9-5M3 16l9 5 9-5',
    arrowRight: 'M5 12h14m-5-5 5 5-5 5',
    object: 'M4 8V4h4m8 0h4v4m0 8v4h-4M8 20H4v-4M8 8h8v8H8Z',
    script: 'M6 3h8l4 4v14H6V3Zm8 0v5h4M10 11l-2 3 2 3m4-6 2 3-2 3',
    collapse: 'm14 7-5 5 5 5',
    expand: 'm10 7 5 5-5 5',
    close: 'm6 6 12 12M6 18 18 6',
    maximize: 'M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5',
    restore: 'M8 8h13v13H8V8ZM3 16V3h13',
    resize: 'm9 19 10-10m-4 10 4-4',
    graph: 'M3 3h6v6H3V3Zm12 12h6v6h-6v-6ZM6 9v9h9M9 6h9v9',
    plus: 'M12 5v14M5 12h14',
    minus: 'M5 12h14',
} as const;

export type EditorIconName = keyof typeof EDITOR_ICONS;

@Component({
    selector: 'editor-icon',
    template: `<svg
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="1.5"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
		focusable="false"
	>
		<path [attr.d]="path()" />
	</svg>`,
    styles: `
		:host {
			display: inline-flex;
			width: var(--layout-icon-size);
			height: var(--layout-icon-size);
			flex: none;
		}
		svg {
			width: 100%;
			height: 100%;
		}
	`,
    host: {'aria-hidden': 'true'},
})
export class EditorIcon {
    readonly name = input.required<EditorIconName>();
    protected readonly path = computed(() => EDITOR_ICONS[this.name()]);
}
