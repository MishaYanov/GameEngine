import {Component, computed, input} from '@angular/core';

export const EDITOR_ICONS = {
    cube: 'm12 3 8 4.5v9L12 21l-8-4.5v-9L12 3Zm0 9L4 7.5m8 4.5 8-4.5M12 12v9',
    play: 'm8 5 12 7-12 7V5Z',
    folder: 'M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z',
    layers: 'm12 3 9 5-9 5-9-5 9-5ZM3 12l9 5 9-5M3 16l9 5 9-5',
    arrowUp: 'M12 19V5m-6 6 6-6 6 6',
    search: 'M16 16l5 5m-3-10a7 7 0 1 1-14 0 7 7 0 0 1 14 0Z',
    refresh: 'M20 7V3m0 4h-4m4 0a9 9 0 1 0 1 8',
    importFile: 'M12 3v12m-4-4 4 4 4-4M4 16v4h16v-4',
    importAudio: 'M3 8v8m4-12v16m4-10v4m4-8v12m4-1v6m-3-3h6',
    audio: 'M3 10v4m4-7v10m5-13v16m5-13v10m4-7v4',
    folderMove: 'M3 19V6h6l2 2h9v3m-1 3 3 3-3 3m-5-3h8',
    openFolder: 'M3 19V6h6l2 2h9v3M3 19l4-8h15l-4 8H3Z',
    locate: 'M12 3v3m0 12v3M3 12h3m12 0h3m-3 0a6 6 0 1 1-12 0 6 6 0 0 1 12 0Z',
    save: 'M4 3h13l3 3v15H4V3Zm4 0v6h8V3M8 21v-7h8v7',
    saveAll: 'M3 17V3h13m-9 4h11l3 3v11H7V7Zm4 0v5h6V7m-6 14v-5h6v5',
    undo: 'M9 5l-5 5 5 5m-5-5h10a5 5 0 0 1 0 10h-2',
    redo: 'm15 5 5 5-5 5m5-5H10a5 5 0 0 0 0 10h2',
    trash: 'M4 7h16m-13 0 1 13h8l1-13M9 7V4h6v3m-4 4v5m2-5v5',
    build: 'M3 7h18v4H3V7Zm2 4v9h14v-9m-9 4h4M5 7l2-3h10l2 3',
    stop: 'M6 6h12v12H6Z',
    hand: 'M8 12V6a1.5 1.5 0 0 1 3 0v5-7a1.5 1.5 0 0 1 3 0v7-5a1.5 1.5 0 0 1 3 0v6-3a1.5 1.5 0 0 1 3 0v6a6 6 0 0 1-6 6h-2a5 5 0 0 1-4-2l-4-5a1.6 1.6 0 0 1 2-2Z',
    move: 'M12 3v18M3 12h18M9 6l3-3 3 3m-6 12 3 3 3-3M6 9l-3 3 3 3m12-6 3 3-3 3',
    rotate: 'M19 7V3m0 4h-4m4 0a8 8 0 1 0 1 9',
    scale: 'M5 11h8v8H5v-8Zm9-1 5-5m-5 0h5v5',
    grid: 'M4 4h16v16H4V4Zm0 5h16M4 15h16M9 4v16m6-16v16',
    snap: 'M5 4h4v9a3 3 0 0 0 6 0V4h4v9a7 7 0 0 1-14 0V4Zm0 5h4m6 0h4',
    edit: 'm4 16 11-11 4 4L8 20H4v-4Zm9-9 4 4m0-8 4 4',
    link: 'm10 14 4-4m-6 6-1 1a3 3 0 0 1-4-4l4-4a3 3 0 0 1 4 0m2 6a3 3 0 0 0 4 0l4-4a3 3 0 0 0-4-4l-1 1',
    copy: 'M8 8h12v12H8V8Zm8-4H4v12',
    arrowLeft: 'M19 12H5m5-5-5 5 5 5',
    arrowRight: 'M5 12h14m-5-5 5 5-5 5',
    object: 'M4 8V4h4m8 0h4v4m0 8v4h-4M8 20H4v-4M8 8h8v8H8Z',
    script: 'M6 3h8l4 4v14H6V3Zm8 0v5h4M10 11l-2 3 2 3m4-6 2 3-2 3',
    inspector: 'M3 5h18v14H3V5Zm10 0v14m3-10h2m-2 5h2',
    agent: 'M5 5h14v11h-7l-4 3v-3H5V5Zm4 6h.1m2.9 0h.1m2.9 0h.1',
    warning: 'M12 4 3 20h18L12 4Zm0 6v4m0 3h.01',
    camera: 'M3 6h12v12H3V6Zm12 4 6-4v12l-6-4',
    collapse: 'm14 7-5 5 5 5',
    expand: 'm10 7 5 5-5 5',
    close: 'm6 6 12 12M6 18 18 6',
    maximize: 'M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5',
    restore: 'M8 8h13v13H8V8ZM3 16V3h13',
    resize: 'm9 19 10-10m-4 10 4-4',
    graph: 'M3 3h6v6H3V3Zm12 12h6v6h-6v-6ZM6 9v9h9M9 6h9v9',
    plus: 'M12 5v14M5 12h14',
    minus: 'M5 12h14',
    lock: 'M7 11h10a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1Zm2 0V7a3 3 0 0 1 6 0v4m-3 4v2',
    unlock: 'M7 11h10a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1Zm2 0V7a3 3 0 0 1 5.8-1m-2.8 9v2',
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
