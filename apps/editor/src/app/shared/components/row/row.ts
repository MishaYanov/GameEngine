import {Component, computed, input, ViewEncapsulation} from '@angular/core';

/** A wrapping control row. Omit columns to fit as many controls as space allows. */
@Component({
    selector: 'editor-row',
    template: '<ng-content />',
    styleUrl: './row.scss',
    // Projected controls also need to shrink within their grid tracks.
    encapsulation: ViewEncapsulation.None,
    host: {'[style.grid-template-columns]': 'templateColumns()'},
})
export class EditorRow {
    readonly columns = input<number>();
    readonly minControlWidth = input('var(--layout-row-control-min-width)');
    protected readonly templateColumns = computed(() => {
        const columns = this.columns();
        return columns === undefined
            ? `repeat(auto-fit, minmax(min(100%, ${this.minControlWidth()}), 1fr))`
            : `repeat(${Number.isFinite(columns) ? Math.max(1, Math.floor(columns)) : 1}, minmax(0, 1fr))`;
    });
}
