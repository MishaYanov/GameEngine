import {Component, computed, effect, inject, input, signal} from '@angular/core';
import {FormControl, FormGroup, ReactiveFormsModule} from '@angular/forms';
import {RouterLink} from '@angular/router';
import {EDITOR_BACKEND} from '../core/editor/editor-backend';
import {EditorService} from '../core/editor/editor.service';
import type {EditorCommand, ModelAsset, SceneEntity} from '../core/editor/editor.models';
import {EditorPane} from '../shared/pane/pane';
import {DemoEditorBackend} from './demo-editor-backend';

@Component({
    selector: 'editor-demo-workspace',
    imports: [EditorPane, ReactiveFormsModule, RouterLink],
    providers: [
        EditorService,
        {provide: EDITOR_BACKEND, useFactory: () => new DemoEditorBackend()},
    ],
    templateUrl: './demo-workspace.html',
    styleUrl: './demo-workspace.scss',
})
export class DemoWorkspace {
    readonly embedded = input(false);
    protected readonly editor = inject(EditorService);
    protected readonly selected = computed(
        () =>
            this.editor.document()?.entities.find((e) => e.key === this.editor.selection()[0]) ?? null,
    );
    protected readonly panel = signal<'assets' | 'activity'>('assets');
    protected readonly activity = signal<string[]>([]);
    protected readonly message = signal('Sample data lives in memory. Reloading resets the project.');
    protected readonly name = new FormControl('', {nonNullable: true});
    protected readonly position = new FormGroup({
        x: new FormControl(0, {nonNullable: true}),
        y: new FormControl(0, {nonNullable: true}),
        z: new FormControl(0, {nonNullable: true}),
    });

    constructor() {
        effect(() => {
            const selected = this.selected();
            this.name.setValue(selected?.name ?? '', {emitEvent: false});
            const [x, y, z] = selected?.transform.position ?? [0, 0, 0];
            this.position.setValue({x, y, z}, {emitEvent: false});
            if (!selected || this.editor.playing()) {
                this.name.disable({emitEvent: false});
                this.position.disable({emitEvent: false});
            } else {
                this.name.enable({emitEvent: false});
                this.position.enable({emitEvent: false});
            }
        });
    }

    async ngOnInit(): Promise<void> {
        await this.reset();
    }

    protected async reset(): Promise<void> {
        try {
            await this.editor.loadCapabilities();
            await this.editor.openProject('demo://editor-playground/project.json');
            await this.editor.assets();
            this.activity.set(['Opened Editor Playground. Native engine is disabled.']);
            this.message.set('Sample project ready. All changes stay in this session.');
        } catch {
            /* The shared editor service exposes the error to the view. */
        }
    }

    protected async run(command: EditorCommand): Promise<void> {
        try {
            await this.editor.execute(command);
            this.activity.update((items) => [...items.slice(-24), command.type.replaceAll('_', ' ')]);
            this.message.set('Sample project updated.');
        } catch {
            /* Error state is rendered below the toolbar. */
        }
    }

    protected rename(event: Event): void {
        event.preventDefault();
        const entity = this.selected();
        if (entity) void this.run({type: 'rename', key: entity.key, name: this.name.value});
    }

    protected applyPosition(): void {
        const entity = this.selected();
        const {x, y, z} = this.position.getRawValue();
        if (!entity) return;
        if (![x, y, z].every(Number.isFinite)) {
            this.message.set('Enter a finite number for each position.');
            return;
        }
        void this.run({
            type: 'set_transforms',
            edits: [{key: entity.key, transform: {...entity.transform, position: [x, y, z]}}],
            space: 'local',
            snap: false,
        });
    }

    protected add(asset: ModelAsset): void {
        void this.run({type: 'spawn', entity: {name: asset.name, model_path: asset.path}});
    }

    protected switchScene(event: Event): void {
        const scene = (event.target as HTMLSelectElement).value;
        void this.run({type: 'switch_scene', scene_id: scene});
    }

    protected async save(): Promise<void> {
        try {
            await this.editor.save();
            this.message.set('Draft saved in memory. Reloading the browser resets all sample data.');
        } catch {
            /* Error state belongs to EditorService. */
        }
    }

    protected objectPosition(entity: SceneEntity, axis: 0 | 2): number {
        return Math.max(12, Math.min(88, 50 + entity.transform.position[axis] * 12));
    }
}
