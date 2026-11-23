import {DOCUMENT} from '@angular/common';
import {afterRenderEffect, Component, computed, ElementRef, inject, input, output, signal, viewChildren} from '@angular/core';
import type {WorkspaceViewKind} from '../../../../core/editor/workspace/workspace-view';
import {EditorIcon, type EditorIconName,} from '../../../../shared/icons/editor-icon';

interface WorkspaceTabDefinition {
    readonly kind: WorkspaceViewKind;
    readonly label: string;
    readonly icon: EditorIconName;
}

const WORKSPACE_TABS: readonly WorkspaceTabDefinition[] = [
    {kind: 'scene-viewport', label: 'Scene', icon: 'cube'},
    {kind: 'game-preview', label: 'Game', icon: 'play'},
    {kind: 'prop-workspace', label: 'Prop', icon: 'object'},
    {kind: 'animation-workspace', label: 'Animation', icon: 'graph'},
    {kind: 'animation-controller', label: 'Animator', icon: 'graph'},
    {kind: 'controls-graph', label: 'Controls Graph', icon: 'graph'},
    {kind: 'audio-workspace', label: 'Audio', icon: 'play'},
    {kind: 'behavior-workspace', label: 'Behavior', icon: 'script'},
];

let nextWorkspaceTabsId = 0;

@Component({
    selector: 'workspace-tabs',
    imports: [EditorIcon],
    templateUrl: './workspace-tabs.html',
    styleUrl: './workspace-tabs.scss',
})
export class WorkspaceTabs {
    readonly activeViewKind = input<WorkspaceViewKind>('scene-viewport');
    readonly availableViews = input<readonly WorkspaceViewKind[]>(['scene-viewport', 'game-preview']);
    readonly viewSelected = output<WorkspaceViewKind>();

    private readonly availableTabs = computed(() =>
        WORKSPACE_TABS.filter(tab => this.availableViews().includes(tab.kind)),
    );
    private readonly activeTab = computed(() =>
        this.availableTabs().find(tab => tab.kind === this.activeViewKind())?.kind ?? this.availableTabs()[0]?.kind,
    );
    protected readonly tabs = computed(() =>
        this.availableTabs().map(tab => ({...tab, active: tab.kind === this.activeTab()})),
    );
    readonly panelId = `workspace-panel-${nextWorkspaceTabsId++}`;
    readonly activeTabId = computed(() => {
        const kind = this.activeTab();
        return kind ? this.buttonId(kind) : null;
    });
    private readonly document = inject(DOCUMENT);
    private readonly focusedTab = signal<WorkspaceViewKind | null>(null);
    protected readonly focusableTab = computed(() => {
        const tabs = this.tabs();
        const focused = this.focusedTab();
        return tabs.find(tab => tab.kind === focused)?.kind ?? tabs.find(tab => tab.active)?.kind ?? tabs[0]?.kind;
    });
    private readonly tabButtons =
        viewChildren<ElementRef<HTMLButtonElement>>('tabButton');

    constructor() {
        afterRenderEffect(() => {
            const focused = this.focusedTab();
            if (!focused || this.availableViews().includes(focused)) return;
            this.focusedTab.set(null);
            const active = this.activeTab();
            if (active && this.document.activeElement === this.document.body) this.focusTab(active);
        });
    }

    protected buttonId(kind: WorkspaceViewKind): string {
        return `${this.panelId}-tab-${kind}`;
    }

    focusTab(kind: WorkspaceViewKind): void {
        this.tabButtons().find(button => button.nativeElement.id === this.buttonId(kind))?.nativeElement.focus();
    }

    protected onTabFocus(kind: WorkspaceViewKind): void {
        this.focusedTab.set(kind);
    }

    protected onTabListFocusOut(event: FocusEvent): void {
        if (!this.tabButtons().some(button => button.nativeElement === event.relatedTarget)) {
            this.focusedTab.set(null);
        }
    }

    protected selectView(kind: WorkspaceViewKind): void {
        const tab = this.tabs().find(tab => tab.kind === kind);

        if (!tab || tab.active) return;

        this.viewSelected.emit(kind);
    }

    protected onTabKeydown(
        event: KeyboardEvent,
        currentKind: WorkspaceViewKind,
    ): void {
        if (event.altKey || event.ctrlKey || event.metaKey) return;

        if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault();
            if (!event.repeat) this.selectView(currentKind);
            return;
        }

        const available = this.tabs();
        const currentIndex = available.findIndex(tab => tab.kind === currentKind);

        if (currentIndex < 0) return;

        let nextIndex: number;

        switch (event.key) {
            case 'ArrowRight':
                nextIndex = (currentIndex + 1) % available.length;
                break;

            case 'ArrowLeft':
                nextIndex =
                    (currentIndex - 1 + available.length) % available.length;
                break;

            case 'Home':
                nextIndex = 0;
                break;

            case 'End':
                nextIndex = available.length - 1;
                break;

            default:
                return;
        }

        const next = available[nextIndex];
        if (!next) return;

        event.preventDefault();
        this.focusTab(next.kind);
    }
}
