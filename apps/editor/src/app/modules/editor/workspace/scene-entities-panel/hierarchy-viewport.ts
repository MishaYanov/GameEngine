import type {HierarchyRow} from './hierarchy-tree';

const OVERSCAN_ROWS = 6;

export function renderHierarchyWindow(
    rows: readonly HierarchyRow[],
    rowHeight: number,
    scrollTop: number,
    viewportHeight: number,
    activeIndex: number | undefined
): readonly HierarchyRow[] {
    const start = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN_ROWS);
    const end = Math.min(rows.length, Math.ceil((scrollTop + viewportHeight) / rowHeight) + OVERSCAN_ROWS);
    const window = rows.slice(start, end);
    // Preserve the active-descendant reference even when manually scrolling.
    if (activeIndex !== undefined && (activeIndex < start || activeIndex >= end)) window.push(rows[activeIndex]);
    return window.sort((a, b) => a.index - b.index);
}

export function hierarchyScrollTop(
    index: number,
    rowHeight: number,
    scrollTop: number,
    viewportHeight: number
): number {
    const top = index * rowHeight;
    if (top < scrollTop) return top;
    const bottom = top + rowHeight;
    return bottom > scrollTop + viewportHeight ? bottom - viewportHeight : scrollTop;
}
