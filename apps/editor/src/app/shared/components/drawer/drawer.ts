import {
	afterNextRender,
	Component,
	computed,
	DestroyRef,
	ElementRef,
	inject,
	Injector,
	input,
	model,
	signal,
} from '@angular/core';
import {BuilderConfig} from '../../builder/models/builder-config.model';
import {Config} from '../../models/config.model';
import {EditorComponentService} from '../../services/editor-component.service';
import {DEFAULT_DRAWER_SETTINGS, DrawerSettings, drawerSettings} from './drawer-settings';
import {Splitter} from '../splitter/splitter';
import {EditorIcon, EditorIconName} from '../../icons/editor-icon';
import {SubjectEntry, SubjectRegistry} from '../../services/subject-registry.service';

export type DrawerType = 'left' | 'right' | 'bottom' | 'top';

@Component({
	selector: 'app-drawer',
	imports: [Splitter, EditorIcon],
	providers: [EditorComponentService, SubjectRegistry],
	templateUrl: './drawer.html',
	styleUrl: './drawer.scss',
	host: {
		role: 'complementary',
		'[attr.aria-label]': 'label()',
		'[attr.data-side]': 'type()',
		'[hidden]': '!isExpanded()',
		'[class.collapsed]': 'collapsed()',
		'[style.--drawer-width]': 'settings().width',
		'[style.--drawer-height]': 'settings().height',
		'[style.--drawer-min-width]': 'settings().minWidth',
		'[style.--drawer-max-width]': 'settings().maxWidth',
		'[style.--drawer-min-height]': 'settings().minHeight',
		'[style.--drawer-max-height]': 'settings().maxHeight',
	},
})
export class Drawer {
	readonly type = input<DrawerType>('left');
	readonly preferenceKey = input<string>();
	readonly label = input('Drawer');
	readonly resizeLabel = input('Resize drawer');
	readonly icon = input<EditorIconName>('layers');
	readonly config = model<Config<DrawerSettings>>({...DEFAULT_DRAWER_SETTINGS});
	readonly builderOptions = input<BuilderConfig>();
	readonly isExpanded = computed(() => this.config().expanded ?? DEFAULT_DRAWER_SETTINGS.expanded);
	readonly collapsed = computed(() => this.config().collapsed ?? DEFAULT_DRAWER_SETTINGS.collapsed);
	protected readonly subjects = inject(SubjectRegistry).subjects;
	protected readonly settings = computed(() => ({...DEFAULT_DRAWER_SETTINGS, ...this.config()}));
	protected readonly horizontal = computed(() => this.type() === 'left' || this.type() === 'right');
	protected readonly size = signal(0);
	protected readonly minimum = signal(0);
	protected readonly maximum = signal(0);
	private readonly injector = inject(Injector);
	private readonly behavior = inject(EditorComponentService);
	readonly configReady = this.behavior.initialized;
	readonly configError = this.behavior.error;
	private readonly element = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;

	constructor() {
		this.behavior.connectConfig<DrawerSettings>(
			() => this.preferenceKey() ?? `drawer:${this.type()}`,
			this.config,
			DEFAULT_DRAWER_SETTINGS,
			drawerSettings,
		);
		const destroyRef = inject(DestroyRef);
		afterNextRender(() => {
			this.measure();
			if (typeof ResizeObserver === 'undefined') return;
			const observer = new ResizeObserver(() => this.measure());
			observer.observe(this.element);
			const parent = this.layoutParent();
			if (parent) observer.observe(parent);
			destroyRef.onDestroy(() => observer.disconnect());
		});
	}

	toggleCollapsed(): void {
		const collapsed = !this.collapsed();
		this.config.update((config) => ({...config, collapsed}));
		afterNextRender(
			() => {
				this.element
					.querySelector<HTMLElement>(collapsed ? '.rail-toggle' : '[data-drawer-toggle]')
					?.focus({preventScroll: true});
			},
			{injector: this.injector},
		);
	}

	protected openSubject(subject: SubjectEntry): void {
		this.config.update((config) => ({...config, collapsed: false}));
		afterNextRender(() => subject.focus(), {injector: this.injector});
	}

	protected resizeBy(delta: number): void {
		this.measure();
		const direction = this.type() === 'right' || this.type() === 'bottom' ? -1 : 1;
		this.setSize(this.size() + delta * direction);
	}

	protected resizeTo(edge: 'min' | 'max'): void {
		this.measure();
		this.setSize(edge === 'min' ? this.minimum() : this.maximum());
	}

	protected resetSize(): void {
		const name = this.horizontal() ? 'width' : 'height';
		const value = this.horizontal() ? 'var(--layout-sidebar-width)' : 'var(--layout-drawer-height)';
		this.config.update((config) => ({...config, [name]: value}));
	}

	private setSize(value: number): void {
		const size = Math.round(Math.max(this.minimum(), Math.min(value, this.maximum())));
		this.size.set(size);
		const dimension = this.horizontal() ? 'width' : 'height';
		if (this.config()[dimension] === `${size}px`) return;
		this.config.update((config) => ({...config, [dimension]: `${size}px`}));
	}

	private measure(): void {
		const horizontal = this.horizontal();
		const dimension = horizontal ? 'width' : 'height';
		const parent = this.layoutParent();
		const parentStyle = parent ? getComputedStyle(parent) : undefined;
		const padding = horizontal
			? (Number.parseFloat(parentStyle?.paddingLeft ?? '') || 0) +
			(Number.parseFloat(parentStyle?.paddingRight ?? '') || 0)
			: (Number.parseFloat(parentStyle?.paddingTop ?? '') || 0) +
			(Number.parseFloat(parentStyle?.paddingBottom ?? '') || 0);
		const available = Math.max(0, (parent?.getBoundingClientRect()[dimension] ?? 0) - padding);
		const style = getComputedStyle(this.element);
		const rootFontSize = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16;
		const pixels = (value: string) => this.pixels(value, available, style, rootFontSize);
		const reserve = pixels(`var(--layout-pane-min-${dimension})`);
		const limit = Math.max(0, available - Math.min(reserve, available * 0.6));
		const settings = this.settings();
		const minimum = Math.min(limit, pixels(horizontal ? settings.minWidth : settings.minHeight));
		const maximum = Math.max(
			minimum,
			Math.min(limit, pixels(horizontal ? settings.maxWidth : settings.maxHeight)),
		);
		this.minimum.set(Math.round(minimum));
		this.maximum.set(Math.round(maximum));
		this.size.set(Math.round(this.element.getBoundingClientRect()[dimension]));
	}

	private layoutParent(): HTMLElement | null {
		let parent = this.element.parentElement;
		while (parent && getComputedStyle(parent).display === 'contents') parent = parent.parentElement;
		return parent;
	}

	private pixels(
		value: string,
		available: number,
		style: CSSStyleDeclaration,
		rootFontSize: number,
		depth = 0,
	): number {
		if (depth > 5) return 0;
		const variable = /^var\((--[a-zA-Z0-9-]+)\)$/.exec(value.trim());
		if (variable)
			return this.pixels(
				style.getPropertyValue(variable[1]).trim(),
				available,
				style,
				rootFontSize,
				depth + 1,
			);
		const amount = Number.parseFloat(value) || 0;
		if (value.endsWith('%')) return (available * amount) / 100;
		if (value.endsWith('rem')) return amount * rootFontSize;
		if (value.endsWith('em')) return amount * (Number.parseFloat(style.fontSize) || rootFontSize);
		if (value.endsWith('vw')) return (innerWidth * amount) / 100;
		if (value.endsWith('vh')) return (innerHeight * amount) / 100;
		return amount;
	}
}
