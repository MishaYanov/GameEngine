import {afterNextRender, Component, computed, ElementRef, input, output, viewChild} from '@angular/core';
import type {CampaignRepairPreview, Collider} from '../../../../core/editor/editor.models';

@Component({
    selector: 'editor-campaign-repair',
    templateUrl: './campaign-repair.html',
})
export class CampaignRepair {
    readonly preview = input.required<CampaignRepairPreview>();
    readonly pending = input(false);
    readonly canApply = input(false);
    readonly keep = output<void>();
    readonly apply = output<void>();
    protected readonly label = computed(() => ({
        key_trigger: 'Key trigger',
        exit_trigger: 'Exit trigger',
        finish_trigger: 'Finish trigger',
    })[this.preview().target.role]);
    private readonly keepButton = viewChild<ElementRef<HTMLButtonElement>>('keepButton');

    constructor() {
        afterNextRender(() => this.keepButton()?.nativeElement.focus());
    }

    protected colliderType(collider: Collider): string {
        return collider.interaction?.sensor ? 'Sensor' : 'Solid';
    }

    protected shape(collider: Collider): string {
        return collider.kind === 'box'
            ? 'Box · half extents ' + collider.half_extents.join(', ')
            : 'Sphere · radius ' + collider.radius;
    }
}
