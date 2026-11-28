import {Component, computed, inject} from '@angular/core';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {ImportedClipKeys} from './imported-clip-keys';
import {AuthoredClipEditor} from './authored-clip-editor';
import {AnimationWorkspaceService} from './animation-workspace.service';

@Component({
    selector: 'editor-animation-workspace',
    imports: [AuthoredClipEditor, EditorSearchPicker, ImportedClipKeys],
    templateUrl: './animation-workspace.html',
    styleUrl: './animation-workspace.scss',
})
export class AnimationWorkspace {
    protected readonly animation = inject(AnimationWorkspaceService);
    protected readonly objectOptions = computed(() => this.animation.options().map(option => ({
        id: option.key,
        label: option.name,
        keywords: option.target.entity_key,
    })));
    protected readonly clipOptions = computed(() => this.animation.clips().map(clip => ({
        id: String(clip.index),
        label: clip.name,
        description: clip.duration_seconds.toFixed(2) + ' s',
    })));
    protected readonly selectedClip = computed(() => String(this.animation.clipIndex()));

    protected chooseClip(value: string): void {
        this.animation.chooseClip(Number(value));
    }
}
