import {Component, computed, input, output} from '@angular/core';
import {EditorSearchPicker} from '../../../../shared/components/search-picker/search-picker';
import {sceneReferenceOptions} from '../scene-reference-options';
import type { ComponentTarget } from '../components/component-draft';

@Component({
	selector: 'editor-reference-bindings',
    imports: [EditorSearchPicker],
	template: `<fieldset [disabled]="disabled()">
		<legend>Scene reference bindings</legend>
		@for (name of names(); track name) {
			<editor-search-picker [label]="name" placeholder="Search scene entities…"
				[options]="entityOptions()" [value]="binding(name)" [clearable]="true"
				[disabled]="disabled()" (picked)="change(name, $event)" />
		} @empty {
			<p>This source has no external reference slots.</p>
		}
	</fieldset>`,
	styleUrl: '../game-settings/settings.scss',
})
export class ReferenceBindings {
	readonly names = input<readonly string[]>([]);
	readonly entities = input<readonly ComponentTarget[]>([]);
	readonly bindings = input<Readonly<Record<string, string>>>({});
	readonly disabled = input(false);
	readonly bindingsChanged = output<Readonly<Record<string, string>>>();
    protected readonly entityOptions = computed(() => sceneReferenceOptions(this.entities()));
	protected binding(name: string): string {
		return Object.hasOwn(this.bindings(), name) ? this.bindings()[name] : '';
	}
	protected change(name: string, value: string): void {
		if (this.disabled()) return;
		const next = { ...this.bindings() };
		Object.defineProperty(next, name, {
			value,
			enumerable: true,
			configurable: true,
			writable: true,
		});
		this.bindingsChanged.emit(next);
	}
}
