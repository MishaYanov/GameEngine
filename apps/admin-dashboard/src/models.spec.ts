import {describe, expect, it} from "vitest";
import {LogBuffer, type LogEntry, type LogPage} from "./models";

const entry = (sequence: number): LogEntry => ({
    schema_version: 1,
    session_id: "fixture",
    sequence,
    timestamp_ms: 100,
    application: "game",
    component: "renderer",
    process_id: 123,
    thread: "main",
    level: "ERROR",
    message: "fixture",
    file: null,
    line: null,
    module: null,
    error: null,
    stack_trace: null,
    correlation: {
        project_id: null,
        operation_id: null,
        build_id: null,
        test_run_id: null,
    },
    attributes: {},
});
const page = (ids: number[], next: number, id = "collector"): LogPage => ({
    collector_id: id,
    entries: ids.map((cursor) => ({cursor, entry: entry(cursor)})),
    next_cursor: next,
    latest_cursor: 9999,
    history_gap: false,
    has_more: true,
    matching_count: ids.length,
});
describe("Log pagination buffer", () => {
    it("uses the returned page cursor without skipping unread records", () => {
        const buffer = new LogBuffer();
        buffer.accept(page([2, 4], 4));
        expect(buffer.cursor).toBe(4);
    });
    it("deduplicates repeated pages", () => {
        const buffer = new LogBuffer();
        buffer.accept(page([1, 2], 2));
        buffer.accept(page([2, 3], 3));
        expect(buffer.entries.map((e) => e.cursor)).toEqual([1, 2, 3]);
    });
    it("resets when the collector changes and retries from zero", () => {
        const buffer = new LogBuffer();
        buffer.accept(page([1], 1));
        expect(buffer.accept(page([50], 50, "new"))).toBe(false);
        expect(buffer.cursor).toBe(0);
        expect(buffer.entries).toEqual([]);
        expect(buffer.accept(page([50], 50, "new"))).toBe(true);
    });
    it("bounds browser history independently of server retention", () => {
        const buffer = new LogBuffer();
        buffer.accept(
            page(
                Array.from({length: 1500}, (_, index) => index + 1),
                1500,
            ),
        );
        expect(buffer.entries).toHaveLength(1000);
        expect(buffer.entries[0].cursor).toBe(501);
    });
    it("preserves history gap warnings until filters reset", () => {
        const buffer = new LogBuffer();
        buffer.accept({...page([1], 1), history_gap: true});
        buffer.accept(page([2], 2));
        expect(buffer.historyGap).toBe(true);
        buffer.reset();
        expect(buffer.historyGap).toBe(false);
    });
});
