import { Injectable } from '@angular/core';
import { invoke, isTauri } from '@tauri-apps/api/core';
import type { DashboardSnapshot, LogFilter, LogEntry } from './models';
@Injectable({ providedIn: 'root' })
export class AdminApi {
  readonly desktop = isTauri();
  readonly preview = !this.desktop && new URLSearchParams(location.search).has('preview');
  async call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
    if (this.desktop) return invoke<T>(command, args);
    if (!this.preview) throw new Error('Desktop services are unavailable. Launch Engine Admin with npm run desktop.');
    if (command === 'dashboard_snapshot') return sampleSnapshot as T;
    if (command === 'log_page') {
      const after = Number(args['after']); const filter = args['filter'] as LogFilter;
      const matching = samples.filter(record => (!filter.operation_id || record.correlation.operation_id === filter.operation_id) && (!filter.severity || record.level === filter.severity) && (!filter.application || record.application === filter.application) && (!filter.component || record.component === filter.component) && (!filter.session_id || record.session_id === filter.session_id) && (!filter.search || record.message.toLowerCase().includes(filter.search.toLowerCase())) && (!filter.from_ms || record.timestamp_ms >= filter.from_ms) && (!filter.to_ms || record.timestamp_ms <= filter.to_ms));
      return { collector_id: 'preview', entries: matching.map((entry) => ({ cursor: entry.sequence, entry })).filter(record => record.cursor > after), next_cursor: samples.length, latest_cursor: samples.length, has_more: false, history_gap: false, matching_count: matching.length } as T;
    }
    throw new Error('Browser preview uses sample data. Desktop actions are disabled.');
  }
}
const now = Date.now();
const samples: LogEntry[] = [
  ['INFO', 'game', 'runtime', 'Engine runtime initialized'],
  ['INFO', 'game', 'renderer', 'Vulkan renderer initialized on the selected device'],
  ['WARNING', 'game', 'renderer::vulkan::validation', 'Swapchain resize requested; waiting for the next frame'],
  ['ERROR', 'game', 'game::renderer', 'render frame: example descriptor allocation failed'],
  ['INFO', 'compiler', 'compiler', 'Compiler process finished'],
].map(([level, application, component, message], index) => ({ schema_version: 1, session_id: `${application}-preview`, sequence: index + 1, timestamp_ms: now - (5 - index) * 2000, application, component, process_id: application === 'game' ? 18420 : 19732, thread: 'main', level: level as LogEntry['level'], message, file: index === 3 ? 'apps/game/src/main.rs' : null, line: index === 3 ? 499 : null, module: null, error: index === 3 ? { category: 'VulkanFrameError', causes: ['Example pool capacity exceeded'] } : null, stack_trace: index === 3 ? { capture_kind: 'reporting_boundary', text: 'Sample stack trace\n  0: game::render_frame\n  1: game::window_event\n  2: winit::event_loop' } : null, correlation: { project_id: null, operation_id: index === 3 ? 'preview-build-failed' : index === 1 ? 'preview-build-passed' : null, build_id: null, test_run_id: null }, attributes: {} }));
const sampleSnapshot: DashboardSnapshot = {
  workspace: 'D:/Coding/Game Engine (sample)', log_directory: 'Local session storage (sample)', data_directory: 'Local admin storage (sample)',
  collector: { storage_error: null, checkpoint_error: null, malformed_lines: 0, history_gaps: 0, oversized_lines: 0, last_scan_ms: now, scanned_files: 2, session_limit_reached: false },
  components: [['runtime', 'Runtime', 'HEALTHY'], ['renderer', 'Renderer', 'DEGRADED'], ['bridge', 'Bridge', 'UNKNOWN'], ['game', 'Game application', 'HEALTHY'], ['compiler', 'Compiler', 'STOPPED'], ['editor-host', 'Client host', 'UNKNOWN']].map(([id, label, health]) => ({ id, label, health: health as DashboardSnapshot['components'][number]['health'], reported_health: health as DashboardSnapshot['components'][number]['health'], last_event_ms: now, last_heartbeat_ms: id === 'game' ? now : null, heartbeat_expected: id === 'game', stale: false, application: id === 'compiler' ? 'compiler' : 'game', session_id: 'game-preview', detail: 'Sample lifecycle state for browser preview' })),
  targets: [['game', 'Game'], ['compiler', 'Compiler'], ['editor-host', 'Client host'], ['vulkan-probe', 'Vulkan probe']].map(([id, label]) => ({ id, label, executable: `target/debug/${id}`, available: false })),
  processes: [{ id: 'sample-game', application: 'game', pid: 18420, session_id: 'game-preview', started_ms: now - 90000, ended_ms: null, state: 'RUNNING', exit_code: null, intentional_stop: false }, { id: 'sample-compiler', application: 'compiler', pid: 19732, session_id: 'compiler-preview', started_ms: now - 8000, ended_ms: now - 4000, state: 'SUCCEEDED', exit_code: 0, intentional_stop: false }],
  cards: [{id:'preview-card',title:'Investigate descriptor allocation',description:'Inspect the renderer diagnostic and rerun the affected build before review.',priority:'High',component:'renderer',column:'InProgress',session_id:'game-preview',jobs:['preview-build-failed','preview-build-passed'],updated_ms:now}],
  jobs: [{id:'preview-build-passed',kind:'BUILD',package:'renderer',profile:'dev',workspace:'Sample workspace',state:'SUCCEEDED',queued_ms:now-30000,started_ms:now-29000,ended_ms:now-25000,process_id:null,exit_code:0,toolchain:'Sample Cargo toolchain',artifacts:[],detail:null},{id:'preview-build-failed',kind:'BUILD',package:'renderer',profile:'dev',workspace:'Sample workspace',state:'FAILED',queued_ms:now-40000,started_ms:now-39000,ended_ms:now-35000,process_id:null,exit_code:101,toolchain:'Sample Cargo toolchain',artifacts:[],detail:null}],
  projects: [], board_storage_error:null, project_storage_error:null,
  process_storage_error: null, process_output_drops: 0,
};
