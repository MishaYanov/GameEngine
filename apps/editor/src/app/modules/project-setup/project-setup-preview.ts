import {Component} from '@angular/core';

@Component({
    selector: 'aside[app-project-setup-preview]',
    template: `
        <div class="preview" role="group" aria-label="Starter project contents">
            <span>YOUR AUTHORING PROJECT</span>
            <ul class="project-contents">
                <li>project.json</li>
                <li>assets/
                    <ul>
                        <li>scenes/ <small>Saved scenes</small></li>
                    </ul>
                </li>
            </ul>
        </div>
        <h2>Project choices</h2>
        <p><strong>Blank project</strong> starts with one scene and a Game Camera.</p>
        <p><strong>Cinder Oath sample</strong> creates an editable copy of the game with its scenes and assets.</p>
    `,
    styleUrl: './project-setup-preview.scss',
})
export class ProjectSetupPreview {}
