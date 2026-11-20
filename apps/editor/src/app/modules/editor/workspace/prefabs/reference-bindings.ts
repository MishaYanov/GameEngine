import { Component, input, output } from '@angular/core';
import type { ComponentTarget } from '../components/component-draft';

@Component({
	selector: 'editor-reference-bindings',
	template: `<fieldset [disabled]="disabled()">
		<legend>Scene reference bindings</legend>
		@for (name of names(); track name) {
			<label
				>{{ name
				}}<select #target [value]="binding(name)" (change)="change(name, target.value)">
					<option value="">Choose scene entity</option>
					@for (entity of entities(); track entity.key) {
						<option [value]="entity.key" [selected]="entity.key === binding(name)">
							{{ entity.name }}
						</option>
					}
				</select></label
			>
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
