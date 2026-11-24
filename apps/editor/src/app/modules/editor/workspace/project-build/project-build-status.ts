import {invoke} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {ProjectBuildResult} from '../../../../core/editor/editor.models';

export type ProjectBuildPhase = 'validating' | 'staging' | 'staged' | 'finishing' | 'succeeded' | 'failed';

type BuildDiagnosticCode = 'validation_failed' | 'player_unavailable' | 'staging_failed' | 'finish_failed' |
    'worker_interrupted';

export interface ProjectBuildStatus {
    readonly build_id: number;
    readonly workspace_generation: number;
    readonly source_revision: number;
    readonly phase: ProjectBuildPhase;
    readonly output: ProjectBuildResult | null;
    readonly diagnostic: {readonly code: BuildDiagnosticCode; readonly message: string} | null;
    readonly retryable: boolean;
}

export interface ProjectBuildReadback {
    readonly occupied: boolean;
    readonly current: ProjectBuildStatus | null;
}

export function readProjectBuildStatus(workspaceGeneration: number): Promise<ProjectBuildReadback> {
    return invoke<ProjectBuildReadback>('editor_project_build_status', {workspaceGeneration});
}

export function watchProjectBuildStatus(refresh: () => void): Promise<() => void> {
    return listen('editor://project-build-changed', () => refresh());
}

export function discardStagedProjectBuild(buildId: number): Promise<void> {
    return invoke('editor_discard_project_build', {buildId});
}
