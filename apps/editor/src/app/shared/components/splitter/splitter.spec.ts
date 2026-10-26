import { ComponentFixture, TestBed } from '@angular/core/testing';
import { vi } from 'vitest';

import { Splitter } from './splitter';

describe('Splitter', () => {
	let component: Splitter;
	let fixture: ComponentFixture<Splitter>;

	beforeEach(async () => {
		await TestBed.configureTestingModule({
			imports: [Splitter],
		}).compileComponents();

		fixture = TestBed.createComponent(Splitter);
		component = fixture.componentInstance;
		await fixture.whenStable();
	});

	it('should create', () => {
		expect(component).toBeTruthy();
	});

	it('resizes by keyboard and supports min/max shortcuts', () => {
		const changes: number[] = [];
		const edges: string[] = [];
		component.resize.subscribe((value) => changes.push(value));
		component.edge.subscribe((value) => edges.push(value));
		const element: HTMLElement = fixture.nativeElement;
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight' }));
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', shiftKey: true }));
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home' }));
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'End' }));
		expect(changes).toEqual([10, -40]);
		expect(edges).toEqual(['min', 'max']);
		expect(element.getAttribute('role')).toBe('separator');
	});

	it('uses up/down keys for horizontal separators', () => {
		fixture.componentRef.setInput('orientation', 'horizontal');
		fixture.detectChanges();
		const changes: number[] = [];
		component.resize.subscribe((value) => changes.push(value));
		const element: HTMLElement = fixture.nativeElement;
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown' }));
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp' }));
		element.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft' }));
		expect(changes).toEqual([10, -10]);
		expect(element.getAttribute('aria-orientation')).toBe('horizontal');
	});

	it('batches pointer moves per frame and flushes the last delta on release', () => {
		let frame!: FrameRequestCallback;
		vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
			frame = callback;
			return 77;
		});
		const cancel = vi.spyOn(window, 'cancelAnimationFrame');
		const element: HTMLElement = fixture.nativeElement;
		element.setPointerCapture = vi.fn();
		element.hasPointerCapture = () => true;
		element.releasePointerCapture = vi.fn();
		const changes: number[] = [];
		component.resize.subscribe((delta) => changes.push(delta));
		const pointer = (name: string, clientX: number) =>
			element.dispatchEvent(Object.assign(new Event(name), { pointerId: 1, button: 0, clientX }));
		pointer('pointerdown', 0);
		pointer('pointermove', 5);
		pointer('pointermove', 12);
		expect(changes).toEqual([]);
		frame(0);
		expect(changes).toEqual([12]);
		pointer('pointermove', 20);
		pointer('pointerup', 20);
		expect(changes).toEqual([12, 8]);
		expect(cancel).toHaveBeenCalledWith(77);
	});
});
