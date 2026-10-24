import {NgComponentOutlet} from '@angular/common';
import {Component, signal} from '@angular/core';
import type {ComponentPreview} from './component-registry';
import {componentRegistry} from './component-registry.generated';

interface Section extends ComponentPreview {
    readonly id: string;
    readonly title: string;
    readonly message?: string;
}

@Component({
    selector: 'editor-design-workbench',
    imports: [NgComponentOutlet],
    templateUrl: './workbench.html',
    styleUrl: './workbench.scss',
})
export class DesignWorkbench {
    protected readonly sections = signal<readonly Section[]>(
        componentRegistry.map(({id, title}) => ({id, title})),
    );

    async ngOnInit(): Promise<void> {
        await Promise.all(
            componentRegistry.map(async (entry) => {
                if (!entry.load) return;
                let result: Section;
                try {
                    const preview = await entry.load();
                    const missing = (preview.requiredInputs ?? entry.requiredInputs).filter(
                        (name) => !Object.hasOwn(preview.inputs ?? {}, name),
                    );
                    result = missing.length
                        ? {
                            id: entry.id,
                            title: entry.title,
                            message: `Set preview inputs in ${entry.id}.design.ts: ${missing.join(', ')}.`,
                        }
                        : {...preview, id: entry.id, title: entry.title};
                } catch {
                    result = {
                        id: entry.id,
                        title: entry.title,
                        message: 'This component preview could not be loaded.',
                    };
                }
                this.sections.update((sections) =>
                    sections.map((section) => (section.id === entry.id ? result : section)),
                );
            }),
        );
    }
}
