import {computed, inject, Injectable, signal} from '@angular/core';
import type {CampaignDefinition, CampaignDiagnosticReport, SceneId} from '../../../../core/editor/editor.models';
import {EditorService} from '../../../../core/editor/editor.service';

interface CheckedScenes {
    readonly generation: number;
    readonly revision: number;
    readonly sceneIds: readonly SceneId[];
}

@Injectable()
export class CampaignRepairQueue {
    private readonly editor = inject(EditorService);
    private readonly lastCheck = signal<CheckedScenes | null>(null);
    readonly currentCheck = computed(() => {
        const check = this.lastCheck();
        return check?.generation === this.editor.workspaceGeneration() ? check : null;
    });
    readonly sceneIds = computed(() => {
        const check = this.currentCheck();
        const snapshot = this.editor.snapshot();
        if (!check || !snapshot) return [];
        const registered = new Set(snapshot.scenes.map(scene => scene.id));
        return check.sceneIds.filter(id => registered.has(id));
    });

    accept(generation: number, definition: CampaignDefinition, report: CampaignDiagnosticReport): void {
        const snapshot = this.editor.snapshot();
        if (generation !== this.editor.workspaceGeneration() || snapshot?.revision !== report.revision) return;
        const registered = new Set(snapshot.scenes.map(scene => scene.id));
        const diagnosed = new Set(report.issues.map(issue => issue.scene_id));
        const campaignOrder = [...definition.chapters.map(chapter => chapter.course_scene), definition.finish_scene];
        const sceneIds = [...new Set(campaignOrder)].filter(id => registered.has(id) && diagnosed.has(id));
        this.lastCheck.set({generation, revision: report.revision, sceneIds});
    }
}
