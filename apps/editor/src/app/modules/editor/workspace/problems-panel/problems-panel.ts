import {Component, computed, input, model, output} from '@angular/core';
import {EditorDialog} from '../../../../shared/components/dialog/dialog';
import type {SceneReferenceLocation} from '../find-usages/scene-references';

export type ProblemLocation = SceneReferenceLocation | { readonly kind: 'build' };

export interface EditorProblem {
    readonly id: string;
    readonly group: 'Scene References' | 'Build';
    readonly label: string;
    readonly detail: string;
    readonly location: ProblemLocation;
    readonly field?: string;
}

@Component({
    selector: 'editor-problems-panel',
    imports: [EditorDialog],
    templateUrl: './problems-panel.html',
    styleUrl: './problems-panel.scss',
})
export class ProblemsPanel {
    readonly open = model(false);
    readonly problems = input<readonly EditorProblem[]>([]);
    readonly available = input(false);
    readonly closedPanel = output<void>();
    readonly revealRequested = output<EditorProblem>();
    protected readonly groups = computed(() => {
        const groups: Map<EditorProblem['group'], EditorProblem[]> = new Map();
        for (const problem of this.problems()) {
            const entries = groups.get(problem.group);
            if (entries) entries.push(problem);
            else groups.set(problem.group, [problem]);
        }
        return [...groups];
    });
    protected readonly config = {width: '38rem', height: '24rem'};
    private pending: EditorProblem | null = null;

    protected reveal(problem: EditorProblem): void {
        if (!this.available() || !this.open()) return;
        this.pending = problem;
        this.open.set(false);
    }

    protected closed(): void {
        const pending = this.pending;
        this.pending = null;
        const current = pending && this.available() ? this.problems().find(problem => problem.id === pending.id) : null;
        this.closedPanel.emit();
        if (current && current === pending) this.revealRequested.emit(current);
    }
}
