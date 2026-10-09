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

# #183 baseline: the current engine, measured 2026-10-09

The misses above were tuned by #185, #186 and #187 (the fragment index, the
kept ignore context, the tiered background merges), and #183 extended the
benchmark to the cases that cost the most. The new baseline was taken on the
same machine with the user's explicit consent (the machine is also used for
games), from `main` at `26c8e224`, `cargo xtask file-index-bench` in release,
two invocations of each tree, `--runs 3`, the default query set. The four raw
reports are beside this file: `generated-450000-183.txt`,
`generated-450000-183-rerun.txt`, `home-183.txt`, `home-183-rerun.txt`.

Conditions: Windows 11 build 26200 (26200.8737), x86_64, i5-14400F, 16 logical
processors, 32 GB, NTFS system drive, a standard user's process, up since
2026-10-06 (no run is cold, as above). Discord, a browser and a few background
apps ran through the runs; the load sat around 40–50%; no other Pane, no other
indexer. The generated tree was made fresh by the first invocation (the
temporary folder held no tree from the earlier runs) and is kept for later
runs; the second invocation reused it.

| Measure | Home (528,275–528,575) | Generated (450,000) | Target (#126) |
| --- | --- | --- | --- |
| First index, run 1 (warm) | 19.53 s, 17.64 s | 10.42 s, 12.42 s | below 12.9 s, aim 6.45 s |
| First index, warm (median) | 12.62 s, 12.76 s | 11.49 s, 11.84 s | below 12.9 s |
| Index on disk | 48.3 MB (91 B/entry) | 44.3 MB (98 B/entry) | below 61.8 MB |
| Query p95, warm, one segment | 24.52 ms, 24.97 ms | 28.18 ms, 25.88 ms | under 10 ms |
| Query p95, several segments + memtable, warm | 48.25 ms, 46.25 ms | 280.42 ms, 259.44 ms | under 10 ms |
| Query p95, while changes arrive and merge | 46.85 ms, 43.60 ms | 229.26 ms, 219.06 ms | under 10 ms |
| Misses (an absent word), p95 | 0.02 ms | 0.02 ms | (a kind, not a target) |
| Typed-out prefixes, p95, one segment | 17.70 ms, 19.89 ms | 21.13 ms, 18.29 ms | (a kind) |
| Kind filter Folder / Document, p95, one segment | 6.24 ms / 142.98 ms, 7.23 ms / 163.74 ms | 11.82 ms / 1209.07 ms, 12.06 ms / 1188.38 ms | (a kind) |
| Re-indexed, near the root, p95 | 6.38 ms, 7.60 ms | 0.30 ms, 0.35 ms | under 10 ms |
| Re-indexed, deep under ignore files, p95 | 0.50 ms, 0.48 ms | 0.66 ms, 0.62 ms | under 10 ms |
| Re-indexed, deep, nothing kept, p95 | 2.15 ms, 1.88 ms | 1.07 ms, 1.82 ms | under 10 ms |
| Catch-up after 10,000 changes | not measured on home | 0.838 s, 0.879 s | under 1 s |
| Opening the index | 5.75 ms, 5.52 ms | 5.25 ms, 6.22 ms | |
| Peak memory while indexing | 282.4 MB, 277.8 MB | 239.6 MB, 242.6 MB | |

Each pair is the two invocations. The home folder grew from 476,017 to about
528,400 entries since 2026-10-08.

**Met** against #126's targets: the first index by its warm median (both
trees; the home tree's run 1, a warm first index of 528k entries, sits at
17.6–19.5 s), the
index size, the changed-file re-index in every row (deep, shallow and with
nothing kept; the timed section ends with a query that finds the change, so
visibility is included), and the catch-up after 10,000 changes — 0.838 s and
0.879 s over about 30,000 journal records, read without administrator rights.

**Missed:** the query p95. One segment sits at 24–28 ms against the 10 ms
target, and several segments with changes in memory at 259–280 ms on the
generated tree. The distribution is wide but stable across the two invocations:
misses cost 0.02 ms (the fragment index answers an absent word from nothing),
most kinds sit at 10–22 ms p95 on one segment, and the kind filter Document
dominates: it asks for more entries and keeps the documents (#183's query set),
measuring 1.19–1.70 s at the 95th percentile on the generated tree and 125–164 ms
on home. The several-segments shape multiplies every kind by about 3 on the
generated tree. Reported as missed; no code was changed by the measurement.
