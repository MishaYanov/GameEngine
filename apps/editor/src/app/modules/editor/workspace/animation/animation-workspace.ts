import {Component, inject} from '@angular/core';
import {AuthoredClipEditor} from './authored-clip-editor';
import {AnimationWorkspaceService} from './animation-workspace.service';

@Component({
    selector: 'editor-animation-workspace',
    imports: [AuthoredClipEditor],
    templateUrl: './animation-workspace.html',
    styleUrl: './animation-workspace.scss',
})
export class AnimationWorkspace {
    protected readonly animation = inject(AnimationWorkspaceService);
}
