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
                        <li>scenes/ <small>Starter scenes</small></li>
                        <li>props/ <small>Models</small></li>
                        <li>textures/ <small>Textures</small></li>
                    </ul>
                </li>
            </ul>
        </div>
        <h2>A project ready to edit.</h2>
        <p>
            Start with sample scenes and reusable assets. Arrange entities, edit their properties,
            and preview your game in the editor.
        </p>
        <ol>
            <li>Create your project folder</li>
            <li>Copy starter scenes and assets</li>
            <li>Open the project in the editor</li>
        </ol>
    `,
    styleUrl: './project-setup-preview.scss',
})
export class ProjectSetupPreview {}
