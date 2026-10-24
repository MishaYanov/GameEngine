import {Component, computed, DestroyRef, inject, signal} from "@angular/core";
import {CommonModule} from "@angular/common";
import {FormsModule} from "@angular/forms";
import {Board} from "./board";
import {Builder} from "./builder";
import {AdminApi} from "./api";
import {
    type CollectedLog,
    type DashboardSnapshot,
    emptyFilter,
    type JobRun,
    LogBuffer,
    type LogFilter,
    type ProcessRun,
} from "./models";

@Component({
    selector: "admin-app",
    standalone: true,
    imports: [CommonModule, FormsModule, Board, Builder],
    templateUrl: "./app.html",
})
export class App {
    readonly api = inject(AdminApi);
    readonly nav = [
        "Overview",
        "Logs",
        "Builds",
        "Tests",
        "Board",
        "Project Builder",
    ];
    readonly section = signal("Overview");
    readonly state = signal<DashboardSnapshot | null>(null);
    readonly logs = signal<CollectedLog[]>([]);
    readonly selected = signal<CollectedLog | null>(null);
    readonly error = signal("");
    readonly notice = signal("");
    readonly paused = signal(false);
    readonly busy = signal("");
    readonly historyGap = signal(false);
    readonly matching = signal(0);
    readonly levels = ["ERROR", "WARNING", "INFO", "DEBUG", "TRACE"];
    readonly runningCount = computed(
        () =>
            this.state()?.processes.filter((run) => run.state === "RUNNING")
                .length ?? 0,
    );
    readonly applications = computed(() =>
        [
            ...new Set(this.logs().map((record) => record.entry.application)),
        ].sort(),
    );
    readonly components = computed(() =>
        [
            ...new Set(this.logs().map((record) => record.entry.component)),
        ].sort(),
    );
    jobPackage = "engine-logger";
    jobProfile = "dev";
    readonly packages = [
        "game",
        "app",
        "compiler",
        "runtime",
        "renderer",
        "bridge",
        "engine-logger",
        "engine-admin",
    ];
    filter: LogFilter = emptyFilter();
    workspace = "";
    logDirectory = "";
    from = "";
    to = "";
    private initialized = false;
    private buffer = new LogBuffer();
    private polling = false;
    private generation = 0;

    constructor() {
        void this.refresh();
        const timer = setInterval(() => void this.refresh(), 1000);
        inject(DestroyRef).onDestroy(() => clearInterval(timer));
    }

    jobRuns(): JobRun[] {
        return (this.state()?.jobs ?? []).filter(
            (run) =>
                run.kind === (this.section() === "Builds" ? "BUILD" : "TEST"),
        );
    }

    queueJob(): void {
        void this.action(
            "queue_job",
            {
                kind: this.section() === "Builds" ? "BUILD" : "TEST",
                package: this.jobPackage,
                profile: this.jobProfile,
            },
            "Queue job",
        );
    }

    linkedLogs(link: { operation_id?: string; session_id?: string }): void {
        this.filter = {...emptyFilter(), ...link};
        this.section.set("Logs");
        this.filtersChanged();
    }

    async createCard(): Promise<void> {
        const entry = this.selected()?.entry;
        if (!entry) return;
        await this.action(
            "save_card",
            {
                id: null,
                draft: {
                    title: entry.message.slice(0, 200),
                    description: JSON.stringify(
                        {
                            source: entry.file,
                            line: entry.line,
                            error: entry.error,
                        },
                        null,
                        2,
                    ),
                    priority: entry.level === "ERROR" ? "High" : "Normal",
                    component: entry.component.slice(0, 100),
                    column: "Backlog",
                    session_id: entry.session_id,
                },
            },
            "Create diagnostic card",
        );
        if (!this.error()) this.section.set("Board");
    }

    selectJob(run: JobRun): void {
        this.filter = emptyFilter();
        this.filter.operation_id = run.id;
        this.section.set("Logs");
        this.filtersChanged();
    }

    async refresh(): Promise<void> {
        if (this.polling) return;
        this.polling = true;
        try {
            const state =
                await this.api.call<DashboardSnapshot>("dashboard_snapshot");
            this.state.set(state);
            if (!this.initialized) {
                this.workspace = state.workspace;
                this.logDirectory = state.log_directory;
                this.initialized = true;
            }
            if (!this.paused()) {
                const generation = this.generation;
                for (let pageCount = 0; pageCount < 8; pageCount++) {
                    const page = await this.api.call<
                        import("./models").LogPage
                    >("log_page", {
                        after: this.buffer.cursor,
                        limit: 500,
                        filter: {...this.filter},
                    });
                    if (generation !== this.generation) break;
                    if (!this.buffer.accept(page)) continue;
                    this.logs.set([...this.buffer.entries].reverse());
                    this.historyGap.set(this.buffer.historyGap);
                    this.matching.set(page.matching_count);
                    if (!page.has_more) break;
                }
            }
        } catch (error) {
            this.error.set(String(error));
        } finally {
            this.polling = false;
        }
    }

    filtersChanged(): void {
        for (const key of [
            "application",
            "component",
            "session_id",
            "search",
            "operation_id",
        ] as const) {
            if (!this.filter[key]?.trim()) this.filter[key] = null;
        }
        this.generation++;
        this.buffer.reset();
        this.logs.set([]);
        this.selected.set(null);
        this.historyGap.set(false);
        this.matching.set(0);
        void this.refresh();
    }

    datesChanged(): void {
        this.filter.from_ms = this.from ? new Date(this.from).getTime() : null;
        this.filter.to_ms = this.to ? new Date(this.to).getTime() : null;
        this.filtersChanged();
    }

    clearFilters(): void {
        this.filter = emptyFilter();
        this.from = "";
        this.to = "";
        this.filtersChanged();
    }

    async action(
        command: string,
        args: Record<string, unknown>,
        label: string,
    ): Promise<void> {
        if (this.busy()) return;
        this.busy.set(label);
        this.error.set("");
        try {
            await this.api.call(command, args);
            await this.refresh();
            this.notice.set(label + " completed");
        } catch (error) {
            this.error.set(String(error));
        } finally {
            this.busy.set("");
        }
    }

    async configure(): Promise<void> {
        if (this.busy()) return;
        this.busy.set("Save workspace");
        try {
            await this.api.call("configure_workspace", {
                workspace: this.workspace,
                logs: this.logDirectory,
            });
            this.filtersChanged();
            this.notice.set("Workspace settings saved");
            await this.refresh();
        } catch (error) {
            this.error.set(String(error));
        } finally {
            this.busy.set("");
        }
    }

    async exportLogs(): Promise<void> {
        try {
            const result = await this.api.call<{ path: string; count: number }>(
                "export_logs",
                {filter: {...this.filter}},
            );
            this.notice.set(
                `Exported ${result.count} retained records to ${result.path}`,
            );
        } catch (error) {
            this.error.set(String(error));
        }
    }

    async copySelected(): Promise<void> {
        if (this.selected()) {
            try {
                await navigator.clipboard.writeText(
                    JSON.stringify(this.selected()!.entry, null, 2),
                );
                this.notice.set("Record copied");
            } catch {
                this.error.set(
                    "Clipboard unavailable; select and copy the record text.",
                );
            }
        }
    }

    running(id: string): ProcessRun | undefined {
        return this.state()?.processes.find(
            (run) => run.application === id && run.state === "RUNNING",
        );
    }

    selectRun(run: ProcessRun): void {
        this.filter.session_id = run.session_id;
        this.section.set("Logs");
        this.filtersChanged();
    }

    duration(run: ProcessRun): string {
        return (
            Math.max(
                0,
                Math.round(
                    ((run.ended_ms ?? Date.now()) - run.started_ms) / 1000,
                ),
            ) + "s"
        );
    }
}
