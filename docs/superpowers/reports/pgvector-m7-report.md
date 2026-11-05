# pgvector conformance after M7

Date: 2026-11-05 · pgrust commit: `c055897146` for the gates and the first scale run. Task 2's run is at `52b5bae361`, and the later samples, `scale-2` and `halfvec-release` ran after it on the same Rust (M7 changes no Rust: `git diff main..HEAD -- crates` is empty) · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded`, Docker `pgvector/pgvector:0.8.7-pg18` and `0.8.1-pg18` by digest · Plan: `docs/superpowers/plans/2026-11-04-pgvector-m7-approximate-differential.md` · Ledger: `~/.cache/pgrust/m7-sdd-ledger.md` · Evidence: `~/.cache/pgrust/m7-evidence/`

## Summary

**The claim:** on real embeddings, pgrust's recall meets spec §8.2's rule against stock C pgvector 0.8.7 for every index type, serial and parallel, and every gated tier is green. That is M7's exit criterion, so Phase 1's milestones are met on `vector/m7`, and Phase 1 closes when the branch lands on `main`. The final whole-branch review found no defect in the tier's measurement or in `run-all.sh`; its corrections to the record are in this report.

**The evidence:**
- **`run-all.sh`: 173/173 in both modes.** The 34 new `recall` rows pass on pgrust and on the seeded C build.
- **The rule,** verbatim from §8.2: `pgrust recall ≥ pgvector recall − 0.01`. It held for all 30 comparisons (recall@10 and @100, 15 cases, 7 type and AM pairs). Recomputing the medians and the rule from `recall.tsv`'s 90 per-build rows gave the same result.
- **The closest row** is `ivfflat_vector_parallel@10`, 0.0042 below C. The medians ranged from −0.0042 to +0.0043, with a mean of −0.0008.
- **A second full-size run** (Task 2, at `52b5bae361`) also passed 34/34, with a worst row of −0.0040 and a mean of −0.00003. The worst rows of the two runs are different cases.
- **The four `exact_*` rows pass:** pgrust's exact answers are within 4e-07 of C's, and identical for `bit`.
- **Every other check passes:** `harness_test.sh` 231 ok, `run-repair-check.sh` 6/6, the client suite 26/26, unit tests, 5 loom models and 4 lints (43 determinism warnings, unchanged). Both reference trees are clean and the capture key is still `8342dca63fe99622`.

**What the claim doesn't say:**
- The rule is one-sided, so it shows pgrust isn't worse than C by more than 0.01. It says nothing about pgrust being better.
- The gate ran at 100,000 rows and 100 dimensions on the `fast-profile` binary. The release-profile binary ran three `vector` cases at 396,000 rows and 300 dimensions, one build per side, as a report, and all 7 rows are `ok` (`scale-2`, server path recorded). That confirms the rule held on a release build. It doesn't measure a gap, because single builds at that size are mostly noise (The scale run).
- Recall is measured on one opclass per type, cosine for `vector`, `halfvec` and `sparsevec` and Hamming for `bit`, so the L2 path has no recall measurement. The diff tier covers the other operators' distance functions.
- The tier can fail by chance: about 6 to 7% per run on the IVFFlat `bit` rows, a rough estimate from 31 builds a side (Recall). Neither full-size run did.
- pgrust builds indexes more slowly than C. On the release-profile binary at gate size, `halfvec` builds take 3.8 to 4.1 times C's time on HNSW and 8.4 to 9.9 times on IVFFlat. The three `vector` cases at scale took 2.1 to 2.9 times C's in `scale-2`. Every other timing is from `fast-profile`, at 1.1 to 1.9 times C's (Build time).

## Gates

The first ten commands ran under `caffeinate -ims` from the repo root, one at a time, on AC power (`pmset -g batt`: AC attached, 80%, not charging, before and after every command). Logs are in `~/.cache/pgrust/m7-evidence/task4/`, each `<label>.log` with its `<label>.out`. The last three ran after the first review, from the controller. `scale-2` ran alongside read-only reviews, so its times aren't clean. `scale-2` and `halfvec-release` logged AC power; the IVFFlat sample logged no power state.

| Command | Verdict | Wall | Log label |
|---|---|---|---|
| `scripts/pgvector/build-pgrust.sh` | built | 35 s | `step1-build` |
| `scripts/pgvector/run-all.sh ref` | **173/173** | 981 s | `step1-run-all-ref`; rows in `run-all-ref/` |
| `scripts/pgvector/run-all.sh pgrust` | **173/173** | 1378 s | `step1-run-all-pgrust`; rows in `run-all-pgrust/` |
| `scripts/pgvector/tests/harness_test.sh` | all passed, **231 ok**, 0 failed | 336 s | `step2-harness` |
| `scripts/pgvector/run-repair-check.sh` | **6/6** | 3 s | `step2-repair` |
| `~/.cache/pgrust/pgvector-rust-check/run.sh 55491` | **26/26** | under 1 s | `step2-client` |
| unit tests, loom, lints (below) | all pass | 115 s | `step3-tests`, `step3/` |
| `cargo build --release --locked --bin postgres` | built | 91 s | `step4-build` |
| `run-recall.sh pgrust`, three cases at scale, first run | 7 of 7 `ok` | 1750 s | `step4-scale`; `../scale/` |
| `scripts/pgvector/run-buildtime.sh pgrust` | a report | 36 s | `step5-buildtime`; `../buildtime/` |
| the same scale command, rerun with the server's path saved | 7 of 7 `ok` | 1782 s | `../scale-2/` |
| `run-recall.sh pgrust`, the four `halfvec` cases, release profile, gate size | 9 of 9 `ok` | 281 s | `../halfvec-release/` |
| `run-recall.sh pgrust`, `ivfflat_vector_serial`, scale size, 15 builds a side | 3 of 3 `ok` | 1730 s | `../ivfflat-scale-sample/` |

- **Tiers, in both modes:** regress 14, TAP 48, bytecmp 29, iterscan 19, diff 22, ondisk 7, recall 34. Each `summary.tsv` was copied to `run-all-<mode>/` straight after the run and counted on disk. Neither report has a `FAIL`, `(no results ...)` or `(run-X.sh exited N)` row.
- **Run-all totals:** the recall tier took about 729 s in `ref` mode and 1038 s in `pgrust` mode, from the gap between `reverse.tsv` and the tier's `summary.tsv` modification times. The plan estimated about 19 minutes for the tier and about 25 for `run-all.sh pgrust`, which took 23.
- **On-disk reverse** (reported, not gated): every row `ok`, `identical` or `clean` in both modes. `clean_pgrust_rows` lists the same three pgrust-only `pg_proc` rows as M6 for `pgrust`, and `none` for `ref`.
- **`harness_test.sh`:** 231 ok is M6's 201 plus the 30 checks with `recall` in their names.
- **Unit tests,** 0 failed in each: `pgvector` 48, `pgvector_f16_parity` 3, `pgvector_hnsw` 13, `pgvector_hnsw_build` 2, `pgvector_hnsw_graph` 16, `pgvector_ivfflat` 8, `pgvector_ivfflat_build` 7, `pgvector_parallel` 1, `types_hnsw` 6, `types_ivfflat` 3 (each `cargo test -p <crate>`), and M6's core crates: `planner` 2 (filtered to `index_workers`), `types_relscan` 3, `execindexing` 18, `catalog_index` 5 (each `--lib`).
- **Loom:** 5 passed. **Lints:** `lint_seam_installs` 1, `lint_determinism` 11, `lint_walker_arms` 1, `lint_inplace_locks` 1. `lint-determinism.sh` prints `PASS (0 violations; 43 warning(s))`.
- **Reference trees:** `git status --porcelain --ignored` on both is empty.
- **Capture key:** `8342dca63fe99622`, both as cached and recomputed from the inputs. The on-disk tier reused the capture: `pgvector-work/ondisk/capture/key` is dated Oct 19 and still matches. Containers were still created during both runs, because the reverse step boots the C image (`run-ondisk.sh:197`, `:255`). None is left.

**Sleep:** no sleep, wake or DarkWake event falls inside any run. The last one in `pmset -g log` is 2026-11-02 11:48:23, three days before this work (`task4/pm-events.txt`; rechecked after the last sample). The wrapper first counted events before and after each command, and that count fell by one across the first scale run because the log drops old entries. A before-and-after count can't be trusted, so the check was redone by date, which is what this paragraph rests on.

**Other load:** other projects' Docker containers stayed up, and load averages were about 3 to 5 with nothing of this task running. Nothing else from this task overlapped a timed run.

## Recall

`run-all.sh pgrust`'s `recall` tier, on the `fast-profile` binary: 100,000 rows and 1,000 held-out queries of GloVe 6B at 100 dimensions (`sparsevec` keeps 16 entries), HNSW `m = 16`, `ef_construction = 64`, `ef_search = 100`, IVFFlat `lists = 316`, `probes = 20`, medians of 3 builds per side. "Sample 2" is the difference in Task 2's separate run of the tier. Both runs pass every row.

| Case | C @10 | pgrust @10 | diff | sample 2 diff | C @100 | pgrust @100 | diff | sample 2 diff |
|---|---|---|---|---|---|---|---|---|
| hnsw_vector_serial | 0.9777 | 0.9795 | +0.0018 | +0.0005 | 0.9244 | 0.9252 | +0.0008 | +0.0006 |
| hnsw_vector_parallel | 0.9809 | 0.9787 | -0.0022 | -0.0033 | 0.9265 | 0.9246 | -0.0019 | -0.0028 |
| hnsw_vector_spill | 0.9791 | 0.9791 | +0.0000 | -0.0018 | 0.9257 | 0.9245 | -0.0012 | -0.0008 |
| hnsw_halfvec_serial | 0.9794 | 0.9774 | -0.0020 | +0.0012 | 0.9243 | 0.9245 | +0.0003 | +0.0003 |
| hnsw_halfvec_parallel | 0.9783 | 0.9789 | +0.0006 | +0.0016 | 0.9247 | 0.9238 | -0.0010 | +0.0022 |
| hnsw_sparsevec_serial | 0.9495 | 0.9479 | -0.0016 | +0.0013 | 0.8373 | 0.8365 | -0.0008 | +0.0001 |
| hnsw_sparsevec_parallel | 0.9479 | 0.9496 | +0.0017 | +0.0017 | 0.8367 | 0.8368 | +0.0000 | +0.0007 |
| hnsw_bit_serial | 0.9779 | 0.9772 | -0.0007 | +0.0001 | 0.9283 | 0.9278 | -0.0005 | +0.0001 |
| hnsw_bit_parallel | 0.9785 | 0.9781 | -0.0004 | -0.0007 | 0.9281 | 0.9278 | -0.0003 | +0.0000 |
| ivfflat_vector_serial | 0.9383 | 0.9367 | -0.0016 | +0.0000 | 0.8864 | 0.8863 | -0.0001 | -0.0019 |
| ivfflat_vector_parallel | 0.9391 | 0.9349 | -0.0042 | +0.0020 | 0.8855 | 0.8841 | -0.0014 | +0.0012 |
| ivfflat_halfvec_serial | 0.9385 | 0.9362 | -0.0023 | -0.0040 | 0.8848 | 0.8838 | -0.0010 | +0.0011 |
| ivfflat_halfvec_parallel | 0.9355 | 0.9364 | +0.0009 | -0.0011 | 0.8817 | 0.8860 | +0.0043 | +0.0003 |
| ivfflat_bit_serial | 0.8740 | 0.8699 | -0.0041 | -0.0024 | 0.7854 | 0.7817 | -0.0037 | -0.0027 |
| ivfflat_bit_parallel | 0.8766 | 0.8745 | -0.0021 | -0.0020 | 0.7849 | 0.7821 | -0.0029 | +0.0078 |

Evidence: `task4/run-all-pgrust/{report.md,recall.tsv}`, `task2/pgrust/`.

Reading it:
- **No overall bias.** The mean difference is −0.0008 in this run and −0.00003 in Task 2's. This run has 8 positive differences out of 30.
- **Two cases were below C in all four comparisons, and neither has a detectable gap.** `ivfflat_bit_serial` read −0.0041 and −0.0037 here, −0.0024 and −0.0027 in Task 2's; `hnsw_vector_parallel` read −0.0022 and −0.0019, then −0.0033 and −0.0028. Two others were above C in all four (`hnsw_vector_serial`, `hnsw_sparsevec_parallel`). Each of those comparisons rests on 3 builds a side, so both low cases were measured again at 31 builds a side (below).
- **Single builds are noisy, most of all IVFFlat `bit`, and equally so on both servers.** Over 31 builds a side, IVFFlat `bit`'s standard deviations are 0.0042 to 0.0057 on C and 0.0037 to 0.0055 on pgrust. Its spans are above the 0.01 margin on every row and on both servers: 0.0164 to 0.0267 on C, 0.0151 to 0.0215 on pgrust.

**Two larger samples,** taken after the gate on the fast-profile binary at this report's code, at full size, 31 builds a side. The mean gap is pgrust minus C. The last column resamples those builds to estimate how often a median of 3 fails the rule by chance. Evidence: `~/.cache/pgrust/m7-evidence/bit-sample/`, `hnsw-parallel-sample/`.

| Row | mean gap | standard error | SD, C | SD, pgrust | span, C | span, pgrust | chance failure |
|---|---|---|---|---|---|---|---|
| ivfflat_bit_serial@10 | +0.0014 | 0.0012 | 0.0057 | 0.0037 | 0.0267 | 0.0185 | about 0.4% |
| ivfflat_bit_serial@100 | −0.0003 | 0.0011 | 0.0042 | 0.0046 | 0.0214 | 0.0208 | about 0.8% |
| ivfflat_bit_parallel@10 | −0.0015 | 0.0012 | 0.0042 | 0.0055 | 0.0204 | 0.0215 | about 4.1% |
| ivfflat_bit_parallel@100 | −0.0016 | 0.0010 | 0.0042 | 0.0039 | 0.0164 | 0.0151 | about 2.1% |
| hnsw_vector_parallel@10 | −0.0001 | 0.0003 | 0.0013 | 0.0013 | 0.0054 | 0.0050 | none in 200,000 resamples |
| hnsw_vector_parallel@100 | −0.0001 | 0.0003 | 0.0010 | 0.0010 | 0.0041 | 0.0044 | none in 200,000 resamples |
| hnsw_vector_serial@10 (control) | +0.0001 | 0.0004 | 0.0015 | 0.0017 | 0.0066 | 0.0067 | none in 200,000 resamples |
| hnsw_vector_serial@100 (control) | +0.0001 | 0.0003 | 0.0012 | 0.0011 | 0.0050 | 0.0049 | none in 200,000 resamples |

- **No detectable gap in either case, well inside the margin.** Every IVFFlat `bit` mean is within 1.6 standard errors of zero, and every HNSW mean within 0.5. The samples exclude gaps beyond about 0.004 at gate size. The streaks in the 3-build runs were noise.
- **The gate itself can fail by chance on the IVFFlat `bit` rows,** at about 6 to 7% per run across the four rows. This is a rough estimate from 31 builds a side. Rows outside `bit` aren't quite zero: `ivfflat_vector_parallel@10` is about 0.5% and `ivfflat_halfvec_serial@10` about 0.1%. A failure on any of these wouldn't by itself mean pgrust regressed.
- **What to do about it is the user's decision,** before the v0.4-beta merge gates on this tier: more builds for the whole tier or for the IVFFlat `bit` cases. `docs/HANDOFF_2026-11-05.md` §1 has the options and what each costs. The rule and margin don't change either way. Until it's decided, a row failure with no bug goes to the user under the plan's stop.

**`ref` mode (C against C):** 34/34, worst row −0.0024. In this run the seeded build's three builds are identical in 10 of the 15 cases, every IVFFlat case and every serial HNSW case, so those medians are a single draw rather than a median of variation; only parallel HNSW and spill vary. That's probably why its IVFFlat rows sit above stock C's together in this run (+0.0019 to +0.0061, apart from one at −0.0006): one fixed k-means result against stock C's varying ones. Task 2's `ref` run had 9 of 15 identical and three negative IVFFlat rows. The `ref` run proves the tier passes C against C; it isn't a measure of C's own spread.

## The scale run

A report, not a gate: 396,000 rows, 300 dimensions, `lists = 630`, `probes = 20`, one build per side. The command, verbatim from the plan: `PGRUST_PROFILE=release PGV_RECALL_ROWS=396000 PGV_RECALL_DIM=300 PGV_RECALL_NNZ=48 PGV_RECALL_LISTS=630 PGV_RECALL_BUILDS=1 scripts/pgvector/run-recall.sh pgrust hnsw_vector_serial hnsw_vector_parallel ivfflat_vector_serial`.

It ran twice. **`scale-2` is the confirmed run:** `scale-2/server-binary.txt` shows the server's command line and its `lsof` text segment, both `target/release/postgres`. The first run's binary stays unconfirmed: it was launched the same way, but no saved file records the server's path. The table is `scale-2`'s, with the first run's figures beside it.

| Case | C | pgrust | difference | verdict | first run's difference |
|---|---|---|---|---|---|
| hnsw_vector_serial@10 | 0.7513 | 0.7489 | −0.0024 | ok | +0.0001 |
| hnsw_vector_serial@100 | 0.6337 | 0.6307 | −0.0030 | ok | +0.0007 |
| hnsw_vector_parallel@10 | 0.7567 | 0.7497 | −0.0070 | ok | +0.0029 |
| hnsw_vector_parallel@100 | 0.6361 | 0.6328 | −0.0032 | ok | +0.0016 |
| ivfflat_vector_serial@10 | 0.7868 | 0.7797 | −0.0071 | ok | −0.0066 |
| ivfflat_vector_serial@100 | 0.7026 | 0.6979 | −0.0047 | ok | −0.0014 |

| Case | C build ms | pgrust build ms | pgrust / C | first run's pgrust / C |
|---|---|---|---|---|
| hnsw_vector_serial | 91224.1 | 188413.4 | 2.07 | 2.08 |
| hnsw_vector_parallel | 42052.8 | 111958.1 | 2.66 | 1.91 |
| ivfflat_vector_serial | 14109.9 | 40408.5 | 2.86 | 3.10 |

`exact_vector` is `ok` in both runs, with a largest difference of 4.9e-07 over 100 queries. No row fell below the rule, so neither run needed the plan's rerun at 3 builds. Evidence: `~/.cache/pgrust/m7-evidence/scale-2/`, `scale/`.

- **This confirms the rule held on a release build. It doesn't measure a gap.** Single-build rows at this size are mostly noise: the two runs' HNSW rows differ by up to 0.0099 between them.
- **`ivfflat_vector_serial` read about −0.007 @10 in both runs, so it was measured at 15 builds a side** (`ivfflat-scale-sample/`): no detectable gap. Mean gaps are +0.0053 @10 and +0.0045 @100, 1.3 and 1.2 standard errors (about 0.004), with pgrust above C on average. The spread is wide on both servers: standard deviation 0.009 on C, 0.012 to 0.013 on pgrust. This sample has no saved server path. It was launched for the release profile, its 40.3 s median build matches `scale-2`'s 40.4 s, and the release binary's access time (03:08:54) falls at its start while `fast-profile`'s didn't change.
- **Recall is lower at this size on both servers,** about 0.75 for HNSW, probably because the search settings stayed at the gate's values while rows and dimensions both grew. That wasn't tested. The two servers track each other, so this isn't a pgrust effect.
- **The build ratios are higher than the gate's** (2.07 to 2.86 in `scale-2`, against 1.48 to 1.66 for the same cases). The 15-build sample's median for `ivfflat_vector_serial` is 2.94. Size, dimensions and binary all differ, and `hnsw_vector_parallel` moved from 1.91 to 2.66 between single builds (`scale-2` ran alongside read-only reviews), so this shows no trend and no cause.
- **Only three cases ran at this size.** `halfvec`, `sparsevec`, `bit`, spill and parallel IVFFlat did not.

## Build time

The repo's `Cargo.toml:912` says numbers come only from release or dist, so the finding below is the release-profile table. The two `fast-profile` tables after it are context, kept because they cover every case and every worker count.

**`halfvec` on the release-profile binary** (gate size, medians of 3 builds a side; server path recorded in `halfvec-release/server-binary.txt`; all 8 recall rows and `exact_halfvec` `ok`):

| Case | C ms | pgrust ms | pgrust / C |
|---|---|---|---|
| hnsw_halfvec_serial | 9109.2 | 37595.5 | 4.13 |
| hnsw_halfvec_parallel | 3384.9 | 12836.9 | 3.79 |
| ivfflat_halfvec_serial | 864.1 | 8509.7 | 9.85 |
| ivfflat_halfvec_parallel | 596.0 | 5028.1 | 8.44 |

**`halfvec` is the outlier.** It's the largest build-time gap M7 measured: about 4× C's on HNSW, 8× to 10× on IVFFlat. The plan's facts put it at about 6×, from one `fast-profile` spike with whole-second rounding that hid IVFFlat's ratio. The ratios are noisy because C's IVFFlat times are small. The cause hasn't been investigated, and it's not fixed in M7. It costs build time only: `halfvec`'s recall rows pass on both binaries.

**Context: per case, in the recall tier, `fast-profile` binary** (GloVe, medians of 3; `run-all.sh pgrust`, and the ratio from Task 2's run; `maintenance_work_mem = 2GB`, or 64MB for spill):

| Case | C ms | pgrust ms | pgrust / C | sample 2 pgrust / C |
|---|---|---|---|---|
| hnsw_vector_serial | 10295.1 | 15769.2 | 1.53 | 1.59 |
| hnsw_vector_parallel | 3757.2 | 5561.4 | 1.48 | 1.51 |
| hnsw_vector_spill | 8985.1 | 15713.4 | 1.75 | 1.79 |
| hnsw_halfvec_serial | 9052.4 | 50832.9 | 5.62 | 5.56 |
| hnsw_halfvec_parallel | 3384.3 | 17055.9 | 5.04 | 5.00 |
| hnsw_sparsevec_serial | 31101.1 | 39253.3 | 1.26 | 1.27 |
| hnsw_sparsevec_parallel | 11131.8 | 13932.5 | 1.25 | 1.21 |
| hnsw_bit_serial | 10349.1 | 12622.1 | 1.22 | 1.26 |
| hnsw_bit_parallel | 3930.7 | 4350.6 | 1.11 | 1.14 |
| ivfflat_vector_serial | 1054.9 | 1749.0 | 1.66 | 1.84 |
| ivfflat_vector_parallel | 671.3 | 1240.5 | 1.85 | 1.55 |
| ivfflat_halfvec_serial | 734.0 | 12265.7 | 16.71 | 13.11 |
| ivfflat_halfvec_parallel | 570.9 | 7178.8 | 12.58 | 9.63 |
| ivfflat_bit_serial | 390.5 | 590.3 | 1.51 | 1.59 |
| ivfflat_bit_parallel | 244.4 | 395.8 | 1.62 | 1.60 |

On this binary `halfvec` reads 5.0× to 5.6× on HNSW, and 9.6× to 16.7× on IVFFlat, where pgrust's times hardly moved between the runs (12.3 s and 11.9 s serial, 7.2 s and 7.1 s parallel) while C's did (734 ms and 905 ms, 571 ms and 736 ms).

**Context: `run-buildtime.sh pgrust`, `fast-profile` binary,** rerun on AC power with the machine awake, as the plan decided, because M6's table was measured through Maintenance Sleep: 100,000 rows of synthetic `vector(128)`, one run per cell. It's a report, not a gate.

| AM | workers | C launched | C ms | pgrust launched | pgrust ms | pgrust / C |
|---|---|---|---|---|---|---|
| hnsw | 0 | 0 | 4233.2 | 0 | 6704.3 | 1.58 |
| hnsw | 1 | 1 | 2547.7 | 1 | 3717.3 | 1.46 |
| hnsw | 2 | 2 | 1906.7 | 2 | 2582.9 | 1.35 |
| hnsw | 4 | 4 | 1370.5 | 4 | 1711.3 | 1.25 |
| ivfflat | 0 | 0 | 325.8 | 0 | 597.4 | 1.83 |
| ivfflat | 1 | 1 | 306.0 | 1 | 459.9 | 1.50 |
| ivfflat | 2 | 2 | 298.4 | 2 | 395.6 | 1.33 |
| ivfflat | 4 | 4 | 282.0 | 4 | 351.8 | 1.25 |

- Every launched count is as forced, and no sleep event falls inside the run (`~/.cache/pgrust/m7-evidence/buildtime/`, `task4/step5-buildtime.log`).
- This replaces M6's table. M6's two wild cells, 0.21× and 18.64×, were sleep: those cells now read 1.58× and 1.35×. Every cell is 1.25× to 1.83×, and the ratio falls as workers rise on both AMs.
- The times fall on both servers as workers rise.
- The synthetic table and GloVe are different data, so the two tables' absolute times don't compare.

## Spill: a worker doing the HNSW overflow flush

M6's coverage list said no run had a worker do the overflow flush. **That gap is closed for this path:** a parallel HNSW `vector` build overflowing at 64 MB. In the gate's `run-all.sh pgrust`, a worker flushed in **6 of 6** builds, and pgrust's own builds flushed in a worker in **3 of 3** (after 58,136, 58,157 and 58,139 tuples; C's after 58,141, 58,158 and 58,110). Each of those builds finished, and `hnsw_vector_spill` passes at +0.0000 and −0.0012.

| Run | Worker flushed, all builds | pgrust's builds | Evidence |
|---|---|---|---|
| `run-all.sh pgrust` | 6 of 6 | 3 of 3 | `task4/run-all-pgrust/` |
| Task 2's `run-recall.sh pgrust` | 4 of 6 | 2 of 3 | `task2/pgrust/` |
| `run-all.sh ref` (stock and seeded C) | 3 of 6 | no pgrust builds | `task4/run-all-ref/` |

So pgrust's builds flushed in a worker in 5 of 6 across the two runs, and the leader did the other. The tier records this rather than forcing it, since which participant overflows is a race, on C too.

Limits:
- It's one type and one memory setting. Other types' overflow in a worker hasn't been seen.
- The `flushed by` column is what the run recorded. The gate run's own logs were replaced by the scale run. Task 2 read logs of both shapes by eye (`\set SHOW_CONTEXT always` is what makes a worker's flush visible).
- No regression test covers the worker path: at toy size the leader always flushes (see the Task 2 minors below).

## What M7 built

- `scripts/pgvector/recall-data.sh` and `recall-prep.awk`: GloVe 6B, downloaded once, sha256 pinned, cached in `~/.cache/pgrust/recall/`, split into base rows and held-out queries.
- `scripts/pgvector/run-recall.sh`: 15 cases, 34 rows, stock C as the baseline, tie-tolerant truth with a `1e-5` epsilon, the median of 3 builds per side. It's the last tier of `run-all.sh` (139 → 173 rows).
- **Guards:** each case dies, leaving no `summary.tsv`, on a failed build, a parallel case built serially, a spill case that doesn't overflow, a plan that isn't an index scan, or a search that returns a row twice. The last was added in review: distance-only scoring can't see a parallel build that indexes a row twice, which is what the parallel cases are there to catch.
- **A baseline outside 0.5 to 0.995 fails the row,** since a comparison there can't detect a difference.
- **`report.md` records what it measured:** a header line gives the subject's binary path, the commit and the data source. Added after the gate, from the final review; it changes output only, so the gate's `report.md` files don't have it.
- **Harness notes:** the seq-scan guard's self-test uses HNSW, because at toy size an IVFFlat scan stays cheapest even with `enable_indexscan=off`. The toy tests run 3 builds, since 1 flaked on noise on C as well.
- **The spec** (§8.2, §8.3, §10) and **CLAUDE.md** are updated, including the capture-key list's missing items, which closes that open thread.
- Task reports: `~/.cache/pgrust/m7-evidence/sdd-reports/`.

## Not run

- **`halfvec`, `sparsevec`, `bit`, spill and parallel IVFFlat at scale.**
- **The full tier on the release-profile binary.** The gate used `fast-profile`. Release-profile runs: the scale runs, the 15-build sample and the four `halfvec` cases.
- **The `dist` profile,** the published artifact.
- **More than two full-size samples of the tier per mode.** Task 2's run predates `run-all.sh`'s wiring. The tier's measurement hasn't changed since it: the only change to its scripts after `52b5bae361` is the output-only `report.md` line, which came after the gate.
- **x86_64, and the wasm build.** macOS arm64 only, as before.
- **Clippy, and a workspace-wide `cargo test`.** M7 changes no Rust.
- **A fresh on-disk capture.** The cached one was reused, as the unchanged key allows.

## What stays open

**Found by M7:**
- **`halfvec` build speed** (Build time): on the release-profile binary about 4× C's on HNSW and 8× to 10× on IVFFlat, the largest build-time gap M7 measured, cause unknown.
- **pgrust's build-time gap against C at scale:** 2.07× to 2.86× in `scale-2`, three single builds (The scale run).
- **The tier's chance failures on IVFFlat `bit`:** about 6 to 7% per run, a rough estimate (Recall). The user decides how many builds to run, before the merge gates on it (`docs/HANDOFF_2026-11-05.md` §1). There's nothing to chase: the 31-build samples show no detectable gap, well inside the margin, for IVFFlat `bit` or `hnsw_vector_parallel`.
- **`PGV_RECALL_*` exported in the environment reach `run-all.sh`,** and nothing in its report shows it. A run with changed sizes or build counts reads like a default one there; the tier's own `report.md` header does show them.
- **M6's report has the same invalid "no container" reasoning** this report corrected under Capture key: it infers reuse from a missing `capture:` line and says no container was created. The conclusion about reuse holds; the container claim doesn't, since the reverse step boots the C image.

**Closed by M7:** a worker doing the overflow flush, for the path above; CLAUDE.md's capture-key list; the rerun of the full `harness_test.sh`, `run-repair-check.sh` and the client suite after M6's final fixes; M6's sleep-affected build-time table; the spec cell's wording minors and `report.md`'s hard-coded "GloVe 6B", in the final review's fixes.

**Deferred review minors** (from the ledger; all may stay open; the final review's other minors are in the ledger too):
- `recall-data.sh:59-66` (Task 1): a corrupt transfer takes the final zip name before the sha check, and a failed curl leaves `glove.6B.zip.part`. Both fail closed.
- `recall-data.sh:85-93` (Task 1): concurrent runs share one `.part`; the harness is serial.
- `recall-data.sh:79-81` (Task 1): an empty stream dies with bash's "No such file" rather than "source is too short".
- `harness_test.sh:1216` (Task 1): the short fixture exercises only `query.tsv`'s count; the drain and a masked unzip failure have no repeatable test.
- Task 2: the worker-flush path has no regression test. Deleting `SHOW_CONTEXT` leaves the suite green; `flush_of` could take a canned log.
- Task 2: a failing `exact_*` row, the verdict boundary and a degraded pgrust subject were checked by hand only.
- `run-recall.sh:360` (Task 2): the `complete=` assignment exits silently under `set -e` if psql fails.
- Spec line 288 (Task 3): the parenthetical says more than the handoff's §3 and omits "never rebase".
- Spec line 250 (Task 3): §8.3's entry order; `recall-prep.awk` is unlisted.

**Carried, unchanged:** `docs/HANDOFF_2026-10-22.md` §3 and §4, apart from the closed items above. That includes M6's other coverage limits: an HNSW row error inside a worker, and loom's reach into `update_connection`'s shrink path and the arena's `OnceLock` publication.
