export type Severity = "ERROR" | "WARNING" | "INFO" | "DEBUG" | "TRACE";
export type Health = "UNKNOWN" | "HEALTHY" | "DEGRADED" | "FAILED" | "STOPPED";
export interface LogEntry {
    schema_version: number;
    session_id: string;
    sequence: number;
    timestamp_ms: number;
    application: string;
    component: string;
    process_id: number;
    thread: string;
    level: Severity;
    message: string;
    file: string | null;
    line: number | null;
    module: string | null;
    error: { category: string; causes: string[] } | null;
    stack_trace: { capture_kind: string; text: string } | null;
    correlation: {
        project_id: string | null;
        operation_id: string | null;
        build_id: string | null;
        test_run_id: string | null;
    };
    attributes: Record<string, string>;
}

export interface CollectedLog {
    cursor: number;
    entry: LogEntry;
}

export interface LogFilter {
    severity: Severity | null;
    application: string | null;
    component: string | null;
    session_id: string | null;
    search: string | null;
    operation_id?: string | null;
    from_ms: number | null;
    to_ms: number | null;
}

export interface LogPage {
    collector_id: string;
    entries: CollectedLog[];
    next_cursor: number;
    latest_cursor: number;
    history_gap: boolean;
    has_more: boolean;
    matching_count: number;
}

export interface Component {
    id: string;
    label: string;
    health: Health;
    reported_health: Health;
    last_event_ms: number | null;
    last_heartbeat_ms: number | null;
    heartbeat_expected: boolean;
    stale: boolean;
    application: string | null;
    session_id: string | null;
    detail: string;
}

export type ProcessStatus =
    "RUNNING" | "SUCCEEDED" | "FAILED" | "STOPPED" | "INTERRUPTED";

export interface ProcessRun {
    id: string;
    application: string;
    pid: number;
    session_id: string | null;
    started_ms: number;
    ended_ms: number | null;
    state: ProcessStatus;
    exit_code: number | null;
    intentional_stop: boolean;
}

export interface LaunchTarget {
    id: string;
    label: string;
    executable: string;
    available: boolean;
}
export interface DashboardSnapshot {
    cards?: Card[];
    board_storage_error?: string | null;
    projects?: Generation[];
    project_storage_error?: string | null;
    jobs?: JobRun[];
    job_storage_error?: string | null;
    workspace: string;
    log_directory: string;
    data_directory: string;
    collector: {
        storage_error: string | null;
        checkpoint_error: string | null;
        malformed_lines: number;
        history_gaps: number;
        oversized_lines: number;
        last_scan_ms: number | null;
        scanned_files: number;
        session_limit_reached: boolean;
    };
    components: Component[];
    processes: ProcessRun[];
    targets: LaunchTarget[];
    process_storage_error: string | null;
    process_output_drops: number;
}

export function emptyFilter(): LogFilter {
    return {
        severity: null,
        application: null,
        component: null,
        session_id: null,
        search: null,
        from_ms: null,
        to_ms: null,
    };
}
export class LogBuffer {
    entries: CollectedLog[] = [];
    cursor = 0;
    collectorId = "";
    historyGap = false;

    reset(): void {
        this.entries = [];
        this.cursor = 0;
        this.collectorId = "";
        this.historyGap = false;
    }

    accept(page: LogPage): boolean {
        if (this.collectorId && this.collectorId !== page.collector_id) {
            this.reset();
            return false;
        }
        this.collectorId = page.collector_id;
        this.historyGap ||= page.history_gap;
        const known = new Set(this.entries.map((record) => record.cursor));
        this.entries = [
            ...this.entries,
            ...page.entries.filter((record) => !known.has(record.cursor)),
        ].slice(-1000);
        this.cursor = page.next_cursor;
        return true;
    }
}

export interface JobRun {
    id: string;
    kind: "BUILD" | "TEST";
    package: string;
    profile: string;
    workspace: string;
    state: string;
    queued_ms: number;
    started_ms: number | null;
    ended_ms: number | null;
    process_id: string | null;
    exit_code: number | null;
    toolchain: string | null;
    artifacts: string[];
    detail: string | null;
}

export type BoardColumn = "Backlog" | "InProgress" | "Blocked" | "Done";

export interface CardDraft {
    title: string;
    description: string;
    priority: string;
    component: string;
    column: BoardColumn;
    session_id: string | null;
}

export interface Card extends CardDraft {
    id: string;
    jobs: string[];
    updated_ms: number;
}

export interface ProjectInput {
    name: string;
    source: string;
    destination: string;
    profile: string;
    target: string;
    offline: boolean;
}

export interface ProjectPreview {
    id: string;
    input: ProjectInput;
    files: { path: string; bytes: number; fingerprint: number }[];
    total_bytes: number;
    excluded: string[];
    template_version: number;
}

export interface Generation {
    id: string;
    name: string;
    destination: string;
    staging: string;
    source: string;
    state: string;
    job_id: string;
    detail: string | null;
    created_ms: number;
    finished_ms: number | null;
    revision: string | null;
    profile: string;
    target: string;
    offline: boolean;
}
