import type {SceneReference} from '../find-usages/scene-references';
import type {EditorProblem} from './problems-panel';

export function editorProblems(
    references: readonly SceneReference[],
    buildFailure: string | null,
): readonly EditorProblem[] {
    const problems = references.flatMap((reference): EditorProblem[] => {
        if (!reference.issue) return [];
        const missing = reference.issue === 'missing-component'
            ? `Component “${reference.targetComponentId}” on entity “${reference.targetKey}” does not exist.`
            : `Entity “${reference.targetKey}” does not exist in this scene.`;
        return [{
            id: reference.id,
            group: 'Scene References',
            label: `${reference.sourceName} · ${reference.field}`,
            detail: missing,
            location: reference.location,
            field: reference.field,
        }];
    });
    if (buildFailure) problems.push({
        id: 'build-failure', group: 'Build', label: 'Game export failed',
        detail: buildFailure, location: {kind: 'build'},
    });
    return problems;
}
