import {Component, computed, DestroyRef, inject, signal} from '@angular/core';
import {DatePipe, DecimalPipe} from '@angular/common';
import {AdminApi} from './api';
import {bytes, deviceState, type MetricsPage} from './metrics.models';

@Component({
    selector: 'resource-metrics',
    imports: [DatePipe, DecimalPipe],
    templateUrl: './metrics.html',
})
export class ResourceMetrics {
    readonly api = inject(AdminApi);
    readonly result = signal<MetricsPage | null>(null);
    readonly snapshot = computed(() => this.result()?.selected ?? null);
    readonly error = signal('');
    readonly selectedId = signal('');
    readonly filter = signal('');
    readonly released = signal(false);
    readonly page = signal(0);
    readonly now = signal(Date.now());
    readonly status = computed(() => this.snapshot() ? deviceState(this.snapshot()!, this.now()) : 'WAITING');
    readonly rows = computed(() => {
        const snapshot = this.snapshot();
        const source = this.released() ? snapshot?.recent_released : snapshot?.resources;
        const query = this.filter().toLowerCase();
        return (source ?? []).filter(row => `${row.kind} ${row.label} ${row.handle} ${row.details}`.toLowerCase().includes(query));
    });
    readonly visibleRows = computed(() => this.rows().slice(this.page() * 100, (this.page() + 1) * 100));
    readonly kinds = computed(() => Object.entries(this.snapshot()?.totals.by_kind ?? {}).filter(([, count]) => count > 0).sort(([a], [b]) => a.localeCompare(b)));
    readonly bytes = bytes;
    private pending = false;
    private destroyed = false;
    private generation = 0;

    constructor() {
        void this.refresh();
        const timer = setInterval(() => {
            this.now.set(Date.now());
            void this.refresh();
        }, 1000);
        inject(DestroyRef).onDestroy(() => {
            this.destroyed = true;
            clearInterval(timer);
        });
    }

    async refresh(): Promise<void> {
        if (this.pending || this.destroyed) return;
        this.pending = true;
        const generation = this.generation;
        try {
            const result = await this.api.call<MetricsPage>('resource_metrics', {selectedId: this.selectedId() || null});
            if (this.destroyed || generation !== this.generation) return;
            this.result.set(result);
            if (result.selected) this.selectedId.set(`${result.selected.session_id}:${result.selected.device_id}`);
            this.now.set(Date.now());
            if (this.page() * 100 >= this.rows().length) this.page.set(0);
            this.error.set('');
        } catch (error: unknown) {
            if (!this.destroyed && generation === this.generation) {
                this.error.set(String(error));
                this.now.set(Date.now());
            }
        } finally {
            this.pending = false;
        }
    }

    selectDevice(event: Event): void {
        this.generation++;
        this.selectedId.set((event.target as HTMLSelectElement).value);
        this.page.set(0);
        void this.refresh();
    }

    search(event: Event): void {
        this.filter.set((event.target as HTMLInputElement).value);
        this.page.set(0);
    }

    showReleased(event: Event): void {
        this.released.set((event.target as HTMLInputElement).checked);
        this.page.set(0);
    }
}
