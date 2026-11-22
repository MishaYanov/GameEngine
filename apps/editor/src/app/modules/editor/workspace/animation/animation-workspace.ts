import {Component, inject} from '@angular/core';
import {ImportedClipKeys} from './imported-clip-keys';
import {AuthoredClipEditor} from './authored-clip-editor';
import {AnimationWorkspaceService} from './animation-workspace.service';

@Component({
    selector: 'editor-animation-workspace',
    imports: [AuthoredClipEditor, ImportedClipKeys],
    templateUrl: './animation-workspace.html',
    styleUrl: './animation-workspace.scss',
})
export class AnimationWorkspace {
    protected readonly animation = inject(AnimationWorkspaceService);
}
