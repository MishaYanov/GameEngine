import {Component, computed, input, output} from '@angular/core';
import type {CampaignDiagnostic, SceneDescriptor, SceneId} from '../../../../core/editor/editor.models';

@Component({
    selector: 'editor-campaign-diagnostics',
    templateUrl: './campaign-diagnostics.html',
})
export class CampaignDiagnostics {
    readonly issues = input.required<readonly CampaignDiagnostic[]>();
    readonly scenes = input.required<readonly SceneDescriptor[]>();
    readonly activeScene = input<SceneId>();
    readonly canNavigate = input(false);
    readonly inspect = output<SceneId>();

    private readonly byId = computed(() => new Map(this.scenes().map(scene => [scene.id, scene.asset_path])));
    protected readonly entries = computed(() => this.issues().map(issue => ({
        ...issue,
        title: issue.scene_id === null ? 'Campaign draft' : this.byId().get(issue.scene_id) ?? issue.scene_id,
        destination: issue.scene_id !== null && issue.scene_id !== this.activeScene() &&
        this.canNavigate() && this.byId().has(issue.scene_id) ? issue.scene_id : null,
    })));
    protected readonly summary = computed(() => {
        const count = this.issues().length;
        if (count === 0) return 'All campaign scenes passed.';
        return count === 1 ? '1 campaign issue.' : count + ' campaign issues.';
    });
}
