import {Component, inject} from '@angular/core';
import {AnimationWorkspaceService} from './animation-workspace.service';

@Component({
    selector: 'editor-animation-workspace',
    templateUrl: './animation-workspace.html',
    styleUrl: './animation-workspace.scss',
})
export class AnimationWorkspace {
    protected readonly animation = inject(AnimationWorkspaceService);
}
