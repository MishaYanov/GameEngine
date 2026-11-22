import {Component, computed, ElementRef, input, output, viewChildren,} from '@angular/core';
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
    readonly propAvailable = input(false);
    readonly viewSelected = output<WorkspaceViewKind>();

    readonly tabs = computed(() =>
        WORKSPACE_TABS.filter(tab => tab.kind !== 'prop-workspace' || this.propAvailable()).map(tab => ({
            ...tab,
            active: tab.kind === this.activeViewKind(),
        })),
    );
    readonly panelId = `workspace-panel-${nextWorkspaceTabsId++}`;
    protected readonly focusableTab = computed(() =>
        this.tabs().find(tab => tab.active)?.kind ?? WORKSPACE_TABS[0]?.kind,
    );
    private readonly tabButtons =
        viewChildren<ElementRef<HTMLButtonElement>>('tabButton');

    protected buttonId(kind: WorkspaceViewKind): string {
        return `${this.panelId}-tab-${kind}`;
    }

    focusTab(kind: WorkspaceViewKind): void {
        this.tabButtons().find(button => button.nativeElement.id === this.buttonId(kind))?.nativeElement.focus();
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
        this.selectView(next.kind);

        this.focusTab(next.kind);
    }
}
