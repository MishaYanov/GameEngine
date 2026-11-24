# Logger queue measurement

On October 3, 2026, a local Windows measurement with Rust 1.97.1, persistence enabled, console disabled, and an 8,192-record queue produced these results. These are single-run observations, not production targets:

| Workload | Debug build | Release build |
| --- | --- | --- |
| 5,000 ordinary records | 14.17 ms | 7.37 ms |
| One error with trace | 19.93 ms | 12.08 ms |
| 20,000 records across four burst threads | 87.60 ms | 61.09 ms |
| Total dropped disk records across the run | 8,391 | 0 |

Both runs reported zero dropped memory records and no storage errors. Burst overload remains possible; choose queue
sizes and verbosity using measurements from the actual client workload. These historical measurements predate resource
snapshots and do not measure their overhead. Engine Admin now consumes the persisted diagnostics and resource snapshots
separately.
