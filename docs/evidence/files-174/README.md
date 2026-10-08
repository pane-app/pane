# #174 evidence: the file index measured on the user's machine

Run on 2026-10-08 with `cargo xtask file-index-bench` from `main` at `ff9ad18d`. Windows 11 build 26200, x86_64, 16 logical processors, NTFS system drive. The process was a standard user's, not elevated. Nothing else indexed during the runs.

- `home.txt`: `--home --runs 3`, the real home folder (read only), 476,017 entries.
- `generated-450000.txt`: `--runs 3`, a generated home-shaped tree of 450,000 entries in the temporary folder. This also runs the catch-up after 10,000 changes through the NTFS change journal.

| Measure | Home (476,017) | Generated (450,000) | Target (#126) |
| --- | --- | --- | --- |
| First index | 12.47 s, 12.72 s, 12.32 s | 9.16 s, 12.60 s, 17.09 s | below 12.9 s, aim 6.45 s |
| Index on disk | 34.7 MB (73 B per entry) | 36.1 MB (80 B per entry) | below 61.8 MB |
| Query p95, warm | 57.42 ms (p50 5.77 ms) | 45.21 ms (p50 18.69 ms) | under 10 ms |
| Changed file re-indexed, p95 | 128.66 ms | 28.88 ms | under 10 ms |
| Catch-up after 10,000 changes | not measured on home | 1.636 s (30,192 records) | under 1 s |
| Opening the index | 5.47 ms | 6.58 ms | |
| Peak memory while indexing | 284.0 MB | 232.0 MB | |

**Cold and warm:** Windows gives a standard user no way to drop the file cache. Run 1 is cold only when it is the first since a restart, and none of these were, so every run is warm. The walk runs at background priority, which is why the generated tree's runs spread from 9 to 17 s.

**Unprivileged journal catch-up:** the generated run's catch-up read the system drive's change journal from this standard user's process (`FSCTL_READ_UNPRIVILEGED_USN_JOURNAL`) and applied 30,192 records. On the same machine, also unelevated, `file_index::journal::tests::the_journal_is_read_without_administrator_rights_and_resolves_to_paths` passed and read the journal; it did not take its no-journal exit.

**Met:** first index below Raycast's 12.9 s; index size about half of Raycast's.

**Missed:**
- query p95, 5–6× the target;
- a changed file's re-index;
- the catch-up's 1 s.

Each missed target is a follow-up to the engine. Reading `store.rs` suggests where the time goes: the inside-word pass over every term when the first pass finds fewer results than asked for, and the per-candidate tombstone scan.
