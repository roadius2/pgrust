# pgvector Phase 1 · M1: `vector` + HNSW at 0.8.7 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring pgrust's existing `vector` type and HNSW access method from pgvector 0.8.5 to 0.8.7 behavior. Fix the two HNSW port bugs the M0 baseline found (TAP 016; TAP 043/044). Prove the result with two new conformance tiers: one compares index files byte for byte, the other compares iterative-scan stop points.

**Architecture:**
- Most changes are small, faithful edits to `crates/contrib/pgvector`, `pgvector_hnsw`, `pgvector_hnsw_build` and `crates/_support/types/types_hnsw`.
- Three changes are structural:
  1. **Opt-in build seed.** Stock C never seeds HNSW builds, so pgrust gets an opt-in seed (the placeholder option `pgrust.hnsw_build_seed`) that mirrors C's `-DHNSW_MEMORY` debug build.
  2. **Pairing heaps.** The search heaps move to the C-exact `pairingheap` crate, so ties pop in C's order.
  3. **Modeled scan memory.** The iterative scan's memory cap stops estimating. A byte-exact model of the scan's C `AllocSet` (`tmpCtx`), plus a port of C's `tidhash` visited set, replace the estimate.
- The harness gains a third C install, `pgvec-seeded`, built with `-DHNSW_MEMORY`. It is the oracle for `run-bytecmp.sh` and `run-iterscan.sh`.

**Tech Stack:** Rust (the pgrust workspace, `fast-profile`), bash 3.2 harness scripts, PostgreSQL 18.6 + pgvector 0.8.7 C reference builds, `pg_regress`, Perl TAP (`prove`), `crates/bin/pagemask`.

**Spec:** `docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md`, corrected 2026-10-05 (§5.1, §5.4, §8.1, §8.2, §8.3, the §9 M1 row). Inputs:
- `docs/superpowers/reports/pgvector-m0-baseline.md`
- `docs/HANDOFF_2026-10-05.md` §1
- the research artifacts in `~/.cache/pgrust/m1-research/`:
  - `iterscan/` has the C allocation trace, the instrumentation patch and the validated model crate;
  - `bytecmp/` has the tested harness scripts, `hnswdiff.py` and the PRNG test-vector programs.

## Global Constraints

- **Branch:** work on `vector/m1`, cut from `main`. Merge locally when done; push only when the user asks. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.
- **Reference trees are read-only:** `crates/pgvector-0.8.7-reference` and `crates/postgres-18.6-reference`. Never edit them and never build inside them. `git status --porcelain --ignored` on both must stay empty.
- **Port conventions** (CLAUDE.md):
  - Keep C function names, and make comments cite C `file.c:line`. pgvector citations point into `crates/pgvector-0.8.7-reference/src/`.
  - Mark intentional differences `DIVERGENCE`.
  - A new error that mirrors a C `ereport` carries `.with_location("<file>.c", <line of the ereport's closing paren>, "<CFunction>")`.
- **Defaults stay stock C:** with `pgrust.hnsw_build_seed` unset, nothing reseeds anything. That option is a placeholder, read through `guc_seams::get_config_option_missing_ok`, and is **never** registered in `guc_tables`.
- **Extension SQL is unchanged in M1:**
  - `crates/contrib/pgvector/extension/vector--0.8.5.sql` and `default_version = '0.8.5'` stay as they are (spec §4.6, §5.1).
  - Upstream's 0.8.5→0.8.7 SQL delta is `COMMENT ON` statements only, and they arrive with the verbatim script in M4.
- **Upstream expected outputs are never edited.**
- **Unit-test command.** It reuses the `target/fast-profile` artifacts the server build produced (`target/debug` is empty, and the disk is about 91% full):
  ```bash
  cargo test --profile fast-profile --locked -p main_main -p pgvector -p pgvector_hnsw -p pgvector_hnsw_build -p types_hnsw --lib -- <name filter>
  ```
  - Listing `main_main` makes feature resolution identical to the server build, so dependencies are not rebuilt. Its own tests are filtered out by name.
  - Below this command is called `UT <filter>`. libtest accepts several filters and runs tests matching any of them.
  - `UT_M1` is every unit test M1 adds or touches: `UT chain_walk vector_combine hnsw_get_max_level level_for_uniform hnsw_check_dim seed_random parse_build_seed candidate_heaps tmpctx:: update_connection real_array_casts vec::`. Never run `UT` with an empty filter, because that also runs `main_main`'s own tests.
- **Cargo.lock:** adding or removing a path dependency changes `Cargo.lock`. After any `Cargo.toml` edit, run `cargo update --workspace --offline` before the next `--locked` command. Then check that `git diff --stat Cargo.lock` touches only that package's dependency list, and commit `Cargo.lock` with the task.
- **Server build:** `scripts/pgvector/build-pgrust.sh` builds `target/fast-profile/postgres` in about 4 minutes cold, less incrementally. Every conformance check needs it rebuilt after a Rust change.
- **Harness commands** (from the repo root):
  - Regression: `scripts/pgvector/run-regress.sh pgrust <test ...>`. Results go to `~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv`, diffs to `regression.diffs`.
  - TAP: `scripts/pgvector/run-tap.sh pgrust <NNN_name.pl ...>`. Logs go under `~/.cache/pgrust/pgvector-work/tap/pgrust/`.
  - `HNSW_TAP` means this list: `010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 019_storage.pl 037_inputs.pl 039_hnsw_cost.pl 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl`.
  - Ports: pgrust 55491, C reference 55492, Docker 55493, seeded C reference 55494 (new).
- **Harness scripts:** `#!/usr/bin/env bash`, `set -euo pipefail`, bash 3.2 compatible. That rules out associative arrays, `${x,,}`, `mapfile`, `sed -i`, and GNU-only flags.
- **Lints** (shell scripts, no build needed), run from the repo root:
  - `bash crates/_support/seams_init/tests/lint-determinism.sh`
  - `bash crates/_support/seams_init/tests/lint-seam-installs.sh`
  Both must pass at the end of every task that touches Rust.

## Review Focus

1. **The seed must stay opt-in.** With `pgrust.hnsw_build_seed` unset, a pgrust HNSW build must not reseed `pg_global_prng_state`; it should behave like stock C, which is unseeded and differs between builds. Test: `test_bytecmp_knob_off` (Task 6) runs the pgrust subject without the `SET` and requires a FAIL against the seeded oracle. `parse_build_seed` unit test (Task 5).
2. **A wrong-dimension query against an *empty* HNSW index.** 0.8.7 errors with `expected 3 dimensions, not 2` before noticing the index is empty. Test: the SQL check in Task 4, Step 6, which diffs the outputs of the two servers.
3. **A NULL `ORDER BY` value** (`ORDER BY v <-> NULL`). C skips `HnswCheckDim` and returns rows, so pgrust must not error. Test: the same SQL check in Task 4, Step 6.
4. **Rescans after an iterative scan hit the memory cap** (nested loop / `LATERAL`). Each rescan must start from a reset `tmpCtx` with fresh visited and discarded state, and stop exactly where C stops. Test: the `lateral_64k` case of `run-iterscan.sh` (Task 9).
5. **A genuinely cyclic page chain** must still raise `ERRCODE_INDEX_CORRUPTED` once the refreshed bound is exceeded. The 016 fix must not turn it into an endless walk. Test: `chain_walk_bound_*` unit tests (Task 1).

---

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `crates/contrib/pgvector_hnsw/src/insert.rs` | page-chain walk bound (016), level draw, `seed_random`, `UpdateNeighborOnDisk`, insert-side `HnswCheckDim` | 1, 3, 4, 5 |
| `crates/contrib/pgvector/src/funcs.rs` | `vector_combine` at 0.8.7 | 2 |
| `crates/contrib/pgvector_hnsw/src/layout.rs` | `HnswGetMaxLevel` cap 63 | 3 |
| `crates/contrib/pgvector_hnsw/src/utils.rs` | `HnswLoadNeighborTids` hardening, `hnsw_check_dim`, candidate pairing heaps, `ElementPool::scan_element`, `search_layer_disk` (heaps, visited `TidHash`, memory charges) | 3, 4, 7, 9 |
| `crates/contrib/pgvector_hnsw/src/vacuum.rs` | caller of `update_neighbors_on_disk` | 3 |
| `crates/contrib/pgvector_hnsw/src/scan.rs` | scan-side `HnswCheckDim`, persistent discarded pairing heap, modeled `tmpCtx`, persistent visited `TidHash` | 4, 7, 9 |
| `crates/contrib/pgvector_hnsw_build/src/lib.rs` | build-side `HnswCheckDim`, `pgrust.hnsw_build_seed`, in-memory search pairing heaps | 4, 5, 7 |
| `crates/contrib/pgvector_hnsw_build/Cargo.toml` | `guc_seams` dependency | 5 |
| `crates/_support/types/types_hnsw/src/lib.rs` | `ScanDiscardedHeap`; `HnswScanOpaqueData` fields | 7, 9 |
| `crates/_support/types/types_hnsw/src/tmpctx.rs` | **new:** `AllocSetModel`, `ListShape`, `TidHash`, C struct sizes | 8 |
| `crates/_support/types/types_hnsw/tests/data/` | **new:** C allocation trace, instrumentation patch, README | 8 |
| `crates/_support/types/types_hnsw/Cargo.toml` | `pairingheap`, `hashfn`; drops `rustc-hash` | 7, 8, 9 |
| `scripts/pgvector/common.sh` | seeded install, port, `pagemask` path, socket-length guard | 6 |
| `scripts/pgvector/build-reference.sh` | step 4: the `pgvec-seeded` install, plus verification | 6 |
| `scripts/pgvector/build-pgrust.sh` | also builds `pagemask` | 6 |
| `scripts/pgvector/run-bytecmp.sh` | **new:** byte-identical HNSW tier | 6 |
| `scripts/pgvector/run-iterscan.sh` | **new:** iterative-scan stop-point tier | 9 |
| `scripts/pgvector/run-all.sh` | adds the two tiers | 6, 9 |
| `scripts/pgvector/tests/harness_test.sh` | self-tests for both tiers | 6, 9 |
| crate headers, spec, `CLAUDE.md`, `docs/superpowers/reports/pgvector-m1-report.md` | docs and the M1 report | 10 |

---

### Task 1: Branch, unit-test command, and the 016 page-chain false positive

**Files:**
- Modify: `crates/contrib/pgvector_hnsw/src/insert.rs:210-233` (walk bound), plus a new `#[cfg(test)] mod tests` at the end of the file

**Interfaces:**
- Produces: `fn chain_walk_exceeds_bound(walked: BlockNumber, max_blocks: &mut BlockNumber, nblocks: impl FnOnce() -> PgResult<BlockNumber>) -> PgResult<bool>`, private to `insert.rs`. Also produces the `#[cfg(test)] mod tests` in `insert.rs` that Tasks 3 and 5 add to.

- [ ] **Step 1: Branch and commit the plan with the corrected spec**

```bash
cd /Users/jody/dev_projects/pgrust
git checkout -b vector/m1
git add docs/superpowers/plans/2026-10-05-pgvector-m1-hnsw-0.8.7.md docs/superpowers/specs/2026-10-04-pgvector-phase1-design.md
git commit -q -F - <<'EOF'
docs(pgvector): M1 plan; correct the Phase 1 spec against v0.8.5..v0.8.7

C seeds HNSW builds only under -DHNSW_MEMORY, vector_combine's real change
is dims-based branching plus CheckDim, sum does not use vector_combine,
and the extension version stays 0.8.5 until M4. Adds the iterative-scan
tier and the seeded reference install.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

- [ ] **Step 2: Prove the unit-test command reuses the fast-profile artifacts**

```bash
du -sh target
cargo test --profile fast-profile --locked -p main_main -p pgvector -p pgvector_hnsw -p pgvector_hnsw_build -p types_hnsw --lib --no-run 2>&1 | tail -5
du -sh target
```
Expected: the build succeeds, `Compiling` lines name only the five listed crates and their test harnesses, and `target` grows by a few GB at most.
- If cargo starts recompiling low-level dependencies (`mcx`, `hashbrown`, `memchr` and the like), stop. The feature-unification assumption failed, so report it before going on.
- Either way, the command is still usable; it just costs more disk.

- [ ] **Step 3: Write the failing tests**

Append to the end of `crates/contrib/pgvector_hnsw/src/insert.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // A stale bound must be refreshed, not reported: a concurrent inserter
    // appended blocks (and chain pages) after the walk read the block count.
    #[test]
    fn chain_walk_bound_refreshes_when_relation_grew() {
        let mut max_blocks: BlockNumber = 3;
        let exceeded = chain_walk_exceeds_bound(3, &mut max_blocks, || Ok(5)).unwrap();
        assert!(!exceeded);
        assert_eq!(max_blocks, 5);
    }

    // A real cycle keeps walking past every refreshed bound.
    #[test]
    fn chain_walk_bound_reports_cycle_when_relation_did_not_grow() {
        let mut max_blocks: BlockNumber = 5;
        assert!(chain_walk_exceeds_bound(5, &mut max_blocks, || Ok(5)).unwrap());
    }

    // Under the bound the block count is not re-read at all.
    #[test]
    fn chain_walk_bound_under_bound_does_not_refresh() {
        let mut max_blocks: BlockNumber = 5;
        let exceeded =
            chain_walk_exceeds_bound(2, &mut max_blocks, || panic!("must not re-read")).unwrap();
        assert!(!exceeded);
        assert_eq!(max_blocks, 5);
    }
}
```

- [ ] **Step 4: Run the tests to see them fail**

Run: `UT chain_walk_bound`
Expected: a compile error, `cannot find function 'chain_walk_exceeds_bound'`.

- [ ] **Step 5: Implement the refreshed bound**

In `crates/contrib/pgvector_hnsw/src/insert.rs`, add this function directly above `// AddElementOnDisk.` (around line 175):
```rust
// Bound on the AddElementOnDisk page-chain walk (pgrust-only; C trusts the
// chain). A cycle-free walk visits distinct blocks that all exist: a page is
// linked only after insert_append_page extended the relation, and HNSW never
// truncates. So once `walked` reaches the current block count, the chain
// revisited a block. The count read before the walk goes stale when
// concurrent inserters extend the relation (and the chain) meanwhile, so it
// is re-read (smgrnblocks asks the file system outside recovery) before
// concluding that.
fn chain_walk_exceeds_bound(
    walked: BlockNumber,
    max_blocks: &mut BlockNumber,
    nblocks: impl FnOnce() -> PgResult<BlockNumber>,
) -> PgResult<bool> {
    if walked < *max_blocks {
        return Ok(false);
    }
    *max_blocks = nblocks()?;
    Ok(walked >= *max_blocks)
}
```
Then, in `add_element_on_disk`, replace the comment block and loop head (currently lines 210-233, from `// The page-chain walk below follows on-disk` through `walked += 1;`) with:
```rust
    // The page-chain walk below follows on-disk `nextblkno` links, which are
    // untrusted (a hostile/corrupt page image can point them into a cycle).
    // chain_walk_exceeds_bound turns a cycle into a catchable index-corruption
    // error instead of an endless INSERT. Each iteration also services pending
    // interrupts so a long walk stays cancellable.
    let mut max_blocks =
        bufmgr::RelationGetNumberOfBlocksInFork(index, ForkNumber::MAIN_FORKNUM)?;
    let mut walked: BlockNumber = 0;

    loop {
        postgres_seams::check_for_interrupts::call()?;
        if chain_walk_exceeds_bound(walked, &mut max_blocks, || {
            bufmgr::RelationGetNumberOfBlocksInFork(index, ForkNumber::MAIN_FORKNUM)
        })? {
            return Err(PgError::error(format!(
                "hnsw index \"{}\" page chain does not terminate (cycle detected)",
                index.name()
            ))
            .with_sqlstate(types_error::ERRCODE_INDEX_CORRUPTED)
            .into());
        }
        walked += 1;
```

- [ ] **Step 6: Run the unit tests**

Run: `UT chain_walk_bound`
Expected: 3 passed.

- [ ] **Step 7: Run TAP 016 against a rebuilt server**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-tap.sh pgrust 016_hnsw_inserts.pl 047_hnsw_vacuum_insert.pl
```
Expected: both `ok` in `~/.cache/pgrust/pgvector-work/tap/pgrust/summary.tsv`. In the M0 baseline, 016 was FAIL.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
git add crates/contrib/pgvector_hnsw/src/insert.rs
git commit -q -F - <<'EOF'
pgvector_hnsw: refresh the page-chain walk bound before reporting a cycle

The bound was read once before the walk; concurrent inserters extend the
relation and the chain meanwhile, so TAP 016 hit "page chain does not
terminate (cycle detected)" on a sound chain. Re-read the block count when
the bound is reached; a real cycle still exceeds the refreshed bound.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 2: `vector_combine` at 0.8.7

**Files:**
- Modify: `crates/contrib/pgvector/src/funcs.rs:561-601` (`fc_vector_combine`), plus a test in its `mod tests` (line 622 on)

**Interfaces:**
- Consumes: `check_dim(usize)` and `check_expected_dim(i32, usize)` (`vec.rs:118-137`), and `StateArray` (`funcs.rs:491-520`).
- Produces: no new names.

- [ ] **Step 1: Write the failing test**

In `crates/contrib/pgvector/src/funcs.rs`, add inside `mod tests` after `real_array_casts_to_vector`:
```rust
    fn combine(m: Mcx<'_>, a: &[f64], b: &[f64]) -> Result<Vec<f64>, String> {
        let mk = |v: &[f64]| {
            let e: Vec<Datum> = v.iter().map(|x| Datum::from_f64(*x)).collect();
            arrayfuncs::construct_array(m, &e, FLOAT8OID, 8, true, b'd').unwrap()
        };
        let (a, b) = (mk(a), mk(b));
        let mut fc = types_fmgr::LocalFcinfo::<2>::new(0);
        // SAFETY: the context outlives the call.
        unsafe { fc.set_result_mcx(m) };
        fc.set_arg(0, Datum::from_usize(a.as_ptr() as usize));
        fc.set_arg(1, Datum::from_usize(b.as_ptr() as usize));
        match fc_vector_combine(None, &mut fc) {
            Ok(d) => {
                let s = StateArray::check(m, d, "test").unwrap();
                Ok((0..s.n_items).map(|i| s.value(i)).collect())
            }
            Err(e) => Err(e.message().to_string()),
        }
    }

    // vector_combine (vector.c:1211-1284, 0.8.7) branches on STATE_DIMS, not
    // on the counts, and CheckDims every non-empty side.
    #[test]
    fn vector_combine_matches_0_8_7() {
        let ctx = mcx::MemoryContext::new("pgvector-test");
        let m = ctx.mcx();
        assert_eq!(combine(m, &[0.0], &[0.0]).unwrap(), vec![0.0]);
        assert_eq!(combine(m, &[1.0, 2.0], &[3.0, 4.0]).unwrap(), vec![4.0, 6.0]);
        // n1 == 0 but dims > 0 (0.8.5 copied s2: {3,4,5}).
        assert_eq!(combine(m, &[0.0, 1.0, 2.0], &[3.0, 4.0, 5.0]).unwrap(), vec![3.0, 5.0, 7.0]);
        // dim1 == 0 but n1 != 0 (0.8.5: "expected 0 dimensions, not 1").
        assert_eq!(combine(m, &[5.0], &[3.0, 4.0]).unwrap(), vec![8.0, 4.0]);
        assert_eq!(combine(m, &[1.0, 2.0], &[3.0, 4.0, 5.0]).unwrap_err(), "expected 1 dimensions, not 2");
        // regress vector_type (expected lines 723-727): 16,001 dimensions.
        let big: Vec<f64> = (1..=16002).map(|n| n as f64).collect();
        for (a, b) in [(&[0.0][..], &big[..]), (&big[..], &[0.0][..]), (&big[..], &big[..])] {
            assert_eq!(combine(m, a, b).unwrap_err(), "vector cannot have more than 16000 dimensions");
        }
    }
```

- [ ] **Step 2: Run the test to see it fail**

Run: `UT vector_combine_matches_0_8_7`
Expected: FAIL. The assertion on line 3 reports `left: [3.0, 4.0, 5.0]`, `right: [3.0, 5.0, 7.0]`.

- [ ] **Step 3: Port the 0.8.7 function**

Replace `fc_vector_combine` (funcs.rs:561-601) with:
```rust
// CreateStateDatums(dim) (vector.c) followed by a copy of one partial state's
// sums; statedatums[0] is filled in by the caller.
fn copy_state_datums<'m>(mcx: Mcx<'m>, s: &StateArray<'_>, dim: usize) -> PgResult<PgVec<'m, Datum>> {
    let mut d: PgVec<'m, Datum> = mcx::vec_with_capacity_in(mcx, dim + 1)?;
    d.push(Datum::null());
    for i in 1..=dim {
        d.push(Datum::from_f64(s.value(i)));
    }
    Ok(d)
}

// vector_combine (vector.c:1211-1284, pgvector 0.8.7; also halfvec_combine's
// symbol). Branches on the partial states' dimensions, CheckDims every
// non-empty side, and sets statedatums[0] = n1 + n2 (vector.c:1275).
pub fn fc_vector_combine(_f: Option<&mut FmgrInfo>, fcinfo: &mut Fcinfo) -> PgResult<Datum> {
    let mcx = fcinfo.result_mcx();
    let s1 = StateArray::check(mcx, fcinfo.arg(0), "vector_combine")?;
    let s2 = StateArray::check(mcx, fcinfo.arg(1), "vector_combine")?;

    let n1 = s1.value(0);
    let n2 = s2.value(0);
    let dim1 = s1.state_dims();
    let dim2 = s2.state_dims();

    let mut datums: PgVec<'_, Datum> = if dim1 == 0 && dim2 == 0 {
        copy_state_datums(mcx, &s1, 0)?
    } else if dim1 == 0 {
        check_dim(dim2)?;
        copy_state_datums(mcx, &s2, dim2)?
    } else if dim2 == 0 {
        check_dim(dim1)?;
        copy_state_datums(mcx, &s1, dim1)?
    } else {
        let dim = dim1;
        check_dim(dim)?;
        check_expected_dim(dim as i32, dim2)?;
        let mut d: PgVec<'_, Datum> = mcx::vec_with_capacity_in(mcx, dim + 1)?;
        d.push(Datum::null());
        for i in 1..=dim {
            let v = s1.value(i) + s2.value(i);
            // Check for overflow
            if v.is_infinite() {
                return Err(Box::new(adt_float::float_overflow_error()));
            }
            d.push(Datum::from_f64(v));
        }
        d
    };
    datums[0] = Datum::from_f64(n1 + n2);
    Ok(image_datum(build_state_array(mcx, &datums)?))
}
```

- [ ] **Step 4: Run the unit test**

Run: `UT vector_combine_matches_0_8_7`
Expected: PASS.

- [ ] **Step 5: Regression tests `vector_type` and `hnsw_vector`**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
cat ~/.cache/pgrust/pgvector-work/regress/pgrust/summary.tsv
```
Expected: `vector_type ok` and `hnsw_vector ok`. In the M0 baseline, `vector_type` was FAIL.

- [ ] **Step 6: Commit**

```bash
git add crates/contrib/pgvector/src/funcs.rs
git commit -q -F - <<'EOF'
pgvector: vector_combine at 0.8.7 (dims-based branches, CheckDim)

Fixes regress vector_type: combining states with more than 16,000
dimensions now raises "vector cannot have more than 16000 dimensions".

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 3: HNSW 0.8.6/0.8.7 hardening — max level 63, the level draw, `UpdateNeighborOnDisk`, `HnswLoadNeighborTids`

**Files:**
- Modify: `crates/contrib/pgvector_hnsw/src/layout.rs:34-41` (`hnsw_get_max_level`), plus a new `#[cfg(test)] mod tests` at the end
- Modify: `crates/contrib/pgvector_hnsw/src/insert.rs:748-756` (`random_level`), `:503-540` (`update_neighbor_on_disk`), `:565-600` (`update_neighbors_on_disk`) and `:728` (caller)
- Modify: `crates/contrib/pgvector_hnsw/src/vacuum.rs:277-279` (caller)
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs:469-473` (`load_neighbor_tids`)

**Interfaces:**
- Produces: `pub fn level_for_uniform(uniform: f64, ml: f64, max_level: i32) -> u8` in `insert.rs`. `random_level(ml, max_level) -> u8` keeps its signature; Task 5's test uses it.
- `update_neighbors_on_disk(index, support, pool, e_id, m, building, op_mcx)` loses its `check_existing: bool` parameter.

- [ ] **Step 1: Write the failing tests**

Append to `crates/contrib/pgvector_hnsw/src/layout.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // HnswGetMaxLevel (hnsw.h:133): (8152 / 6 / m) - 2, capped at 63 since 0.8.6.
    #[test]
    fn hnsw_get_max_level_caps_at_63() {
        assert_eq!(hnsw_get_max_level(2), 63);
        assert_eq!(hnsw_get_max_level(16), 63);
        assert_eq!(hnsw_get_max_level(20), 63);
        assert_eq!(hnsw_get_max_level(21), 62);
        assert_eq!(hnsw_get_max_level(100), 11);
    }
}
```
Add inside `mod tests` in `insert.rs` (created in Task 1):
```rust
    // HnswInitElement (hnswutils.c:250-255, 0.8.7): uniform == 0.0 takes maxLevel.
    #[test]
    fn level_for_uniform_matches_c() {
        let ml = hnsw_get_ml(16);
        assert_eq!(level_for_uniform(0.0, ml, 63), 63);
        assert_eq!(level_for_uniform(1e-10, ml, 63), 8);
        assert_eq!(level_for_uniform(0.5, ml, 63), 0);
        assert_eq!(level_for_uniform(1e-300, ml, 63), 63);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `UT hnsw_get_max_level_caps_at_63` and `UT level_for_uniform_matches_c`
Expected:
- The first fails with `left: 255, right: 63`.
- The second fails to compile: `cannot find function 'level_for_uniform'`.
- Until `level_for_uniform` exists, the compile error hides the first failure. That's fine.

- [ ] **Step 3: Cap at 63 and make the level draw explicit**

In `layout.rs`, replace the `// HnswGetMaxLevel (hnsw.h) — integer division order preserved.` comment and the final expression of `hnsw_get_max_level`:
```rust
// HnswGetMaxLevel (hnsw.h:133, capped at 63 since 0.8.6) — integer division
// order preserved.
pub fn hnsw_get_max_level(m: i32) -> i32 {
    let v = (BLCKSZ - SIZE_OF_PAGE_HEADER - HNSW_PAGE_OPAQUE_SIZE - NEIGHBOR_TIDS_OFFSET
        - SIZE_OF_ITEM_ID)
        / SIZE_OF_ITEM_POINTER
        / (m as usize);
    ((v as i32) - 2).min(63)
}
```
Leave `scan.rs`'s `HNSW_MAX_ENTRY_LEVEL = 255` alone: indexes written by 0.8.5 can legally hold levels above 63.

In `insert.rs`, replace `random_level` (lines 748-756) with:
```rust
// HnswInitElement's level draw (hnswutils.c:250-255): a uniform of exactly
// 0.0 takes maxLevel instead of computing -log(0); then the maxLevel cap.
pub fn level_for_uniform(uniform: f64, ml: f64, max_level: i32) -> u8 {
    let mut level = if uniform == 0.0 {
        max_level
    } else {
        (-(uniform.ln()) * ml) as i32
    };
    if level > max_level {
        level = max_level;
    }
    level as u8
}

// RandomDouble() (hnsw.h:105): pg_prng_double(&pg_global_prng_state).
pub fn random_level(ml: f64, max_level: i32) -> u8 {
    level_for_uniform(pg_prng::global_prng(|p| p.next_f64()), ml, max_level)
}
```

- [ ] **Step 4: Always check for an existing connection (upstream #1010)**

In `insert.rs`:
- Delete the `check_existing: bool,` parameter of `update_neighbor_on_disk` (line 513).
- Replace line 532, `if check_existing && connection_exists(pool, new_element, ntup_bytes, start_idx, lm) {`, with:
  ```rust
      // Check for existing connection (hnswinsert.c:503-505; upstream #1010).
      if connection_exists(pool, new_element, ntup_bytes, start_idx, lm) {
  ```
- Delete the `check_existing: bool,` parameter of `update_neighbors_on_disk` (line 572).
- Change its inner call (line 597) to `pool, hc.element, e_id, idx, m, lm, lc, index, building, op_mcx,`.
- Change the caller at line 728 to `update_neighbors_on_disk(index, support, pool, element, m, building, op_mcx)?;`.

In `vacuum.rs:277-279`, change the call to:
```rust
    crate::insert::update_neighbors_on_disk(
        vs.index, &mut support, pool, e_id, vs.m, false, vs.op_mcx,
    )
```

- [ ] **Step 5: `HnswLoadNeighborTids` start through `mul_size`**

In `utils.rs` `load_neighbor_tids`, replace `let start = (e.level as i32 - lc) * m;` and the loop that uses it (lines 469-473) with:
```rust
    // start = mul_size(element->level - lc, m) (hnswutils.c:789, 0.8.6+). A
    // level below lc (corrupt metapage entry level) converts to a huge Size,
    // so mul_size raises instead of indexing out of bounds.
    let start = match mcx::mul_size((e.level as i32 - lc) as i64 as usize, m as usize) {
        Ok(s) => s,
        Err(err) => {
            UnlockReleaseBuffer(buf)?;
            return Err(err);
        }
    };
    for i in 0..lm as usize {
        indextids[i].copy_from_slice(ntup.indextid_bytes(start + i));
    }
```
This path is reachable only through a corrupt metapage. There's no buffer-level unit test for it; review and the HNSW TAP run below cover it.

- [ ] **Step 6: Run the unit tests**

Run: `UT hnsw_get_max_level_caps_at_63`, `UT level_for_uniform_matches_c`, then `UT chain_walk_bound`.
Expected: all pass.

- [ ] **Step 7: HNSW TAP regression check**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
scripts/pgvector/run-tap.sh pgrust 010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl
```
Expected: both regression tests `ok` and all 9 TAP tests `ok`.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
git add crates/contrib/pgvector_hnsw/src/{layout,insert,vacuum,utils}.rs
git commit -q -F - <<'EOF'
pgvector_hnsw: 0.8.6/0.8.7 hardening (max level 63, level draw, #1010)

HnswGetMaxLevel caps at 63; the uniform == 0.0 level draw is explicit;
UpdateNeighborOnDisk always checks for an existing connection; the
HnswLoadNeighborTids start offset goes through mul_size.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 4: `HnswCheckDim` at build, insert and scan

**Files:**
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs` (new `hnsw_check_dim` after `check_type_supported`, around line 77; new `#[cfg(test)] mod tests` at the end)
- Modify: `crates/contrib/pgvector_hnsw/src/insert.rs:772` (after `let meta = read_meta(index)?;`)
- Modify: `crates/contrib/pgvector_hnsw/src/scan.rs:222-232` (`get_scan_items`, before the `meta_entry_point` early return)
- Modify: `crates/contrib/pgvector_hnsw_build/src/lib.rs:822-828` (`insert_tuple`, after `form_index_value`)

**Interfaces:**
- Produces: `pub fn hnsw_check_dim(expected: i32, collation: Oid, value: Datum) -> PgResult<()>` in `pgvector_hnsw::utils`.

- [ ] **Step 1: Write the failing unit test**

Append to `crates/contrib/pgvector_hnsw/src/utils.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // HnswCheckDim (hnswutils.c:1366-1374): ERRCODE_DATA_EXCEPTION, C's text.
    #[test]
    fn hnsw_check_dim_matches_c() {
        let ctx = mcx::MemoryContext::new("hnsw-test");
        let m = ctx.mcx();
        let mut b = pgvector::vec::VecBuilder::new(m, 2).unwrap();
        b.set(0, 1.0);
        b.set(1, 2.0);
        let img = b.image();
        let d = Datum::from_usize(img.as_ptr() as usize);
        assert!(hnsw_check_dim(2, types_core::InvalidOid, d).is_ok());
        let e = hnsw_check_dim(3, types_core::InvalidOid, d).unwrap_err();
        assert_eq!(e.message(), "expected 3 dimensions, not 2");
        assert_eq!(e.sqlstate(), types_error::ERRCODE_DATA_EXCEPTION);
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `UT hnsw_check_dim_matches_c`
Expected: a compile error, `cannot find function 'hnsw_check_dim'`.

- [ ] **Step 3: Implement `hnsw_check_dim`**

In `utils.rs`, below `check_type_supported`:
```rust
// HnswCheckDim (hnswutils.c:1366-1374, pgvector 0.8.7). typeInfo->dimensions
// is vector_dims for the vector opclasses (hnswutils.c:1407); the halfvec/
// bit/sparsevec type info arrives in M3.
pub fn hnsw_check_dim(expected: i32, collation: Oid, value: Datum) -> PgResult<()> {
    let dim = types_fmgr::fcinfo::direct_function_call1_coll(
        pgvector::funcs::fc_vector_dims,
        collation,
        value,
    )?
    .as_i32();
    if dim != expected {
        return Err(PgError::error(format!("expected {expected} dimensions, not {dim}"))
            .with_sqlstate(types_error::ERRCODE_DATA_EXCEPTION)
            .with_location("hnswutils.c", 1373, "HnswCheckDim")
            .into());
    }
    Ok(())
}
```
Add `Oid` to `utils.rs`'s `types_core` import if it isn't there.

- [ ] **Step 4: Call it where C does**

- **Insert.** In `insert.rs` `insert_tuple_on_disk`, directly after `let meta = read_meta(index)?;` (line 772):
  ```rust
      // Check dimensions match index (hnswinsert.c:716-717).
      crate::utils::hnsw_check_dim(
          meta.dimensions as i32,
          support.collation,
          Datum::from_usize(value.as_ptr() as usize),
      )?;
  ```
- **Scan.** In `scan.rs` `get_scan_items`, directly after the `validate_meta_fields(...)?;` line and before `so.m = meta.m as i32;`:
  ```rust
      // Check dimensions match index (hnswscan.c:42-44): only for a non-NULL
      // value, and before the empty-index return below.
      if let Some(v) = so.value.as_ref() {
          hnsw_check_dim(
              meta.dimensions as i32,
              so.support.collation,
              Datum::from_usize(v.as_ptr() as usize),
          )?;
      }
  ```
- **Build.** In `pgvector_hnsw_build/src/lib.rs` `insert_tuple`, directly after `bs.support = support;`, which follows the `form_index_value` match (line 826):
  ```rust
      // Check dimensions match index (hnswbuild.c:503-504): the column typmod.
      hnsw_check_dim(bs.dimensions, bs.support.collation, Datum::from_usize(img.as_ptr() as usize))?;
  ```

- [ ] **Step 5: Run the unit test**

Run: `UT hnsw_check_dim_matches_c`
Expected: PASS.

- [ ] **Step 6: Compare the SQL behaviour of both servers (Review Focus 2 and 3)**

```bash
scripts/pgvector/build-pgrust.sh
w="$(mktemp -d)"
cat >"$w/dim.sql" <<'SQL'
CREATE EXTENSION vector;
CREATE TABLE t (val vector(3));
CREATE INDEX ON t USING hnsw (val vector_l2_ops);
SET enable_seqscan = off;
SELECT * FROM t ORDER BY val <-> '[1,2]' LIMIT 1;
INSERT INTO t VALUES ('[1,2,3]'), ('[4,5,6]');
SELECT * FROM t ORDER BY val <-> '[1,2]' LIMIT 1;
SELECT count(*) FROM (SELECT * FROM t ORDER BY val <-> NULL LIMIT 5) s;
SELECT * FROM t ORDER BY val <-> '[1,2,3]' LIMIT 1;
SQL
for s in ref pgrust; do
  scripts/pgvector/server.sh fresh "$s" >/dev/null
  scripts/pgvector/server.sh psql "$s" -v ON_ERROR_STOP=0 -f "$w/dim.sql" >"$w/out.$s" 2>&1 || true
  scripts/pgvector/server.sh stop "$s"
done
diff "$w/out.ref" "$w/out.pgrust" && echo identical
grep -c 'expected 3 dimensions, not 2' "$w/out.pgrust"
```
Expected: `identical`, then `2`. The first match is the empty-index query. The NULL query returns `2` on both servers.

- [ ] **Step 7: Regression and TAP check, lints, commit**

```bash
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
scripts/pgvector/run-tap.sh pgrust 013_hnsw_vector_insert_recall.pl 017_hnsw_filtering.pl 037_inputs.pl
bash crates/_support/seams_init/tests/lint-determinism.sh
git add crates/contrib/pgvector_hnsw/src/{utils,insert,scan}.rs crates/contrib/pgvector_hnsw_build/src/lib.rs
git commit -q -F - <<'EOF'
pgvector_hnsw: HnswCheckDim at build, insert and scan (0.8.7)

A query vector whose dimensions differ from the index now raises
"expected N dimensions, not M" (22000), even on an empty index, as in
pgvector 0.8.7.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```
Expected: regression and TAP all `ok`.

---

### Task 5: Opt-in build seed `pgrust.hnsw_build_seed`

**Files:**
- Modify: `crates/contrib/pgvector_hnsw/src/insert.rs` (new `seed_random` next to `random_level`; a test in `mod tests`)
- Modify: `crates/contrib/pgvector_hnsw_build/src/lib.rs` (the `use` line 9; new `hnsw_build_seed`/`parse_build_seed`; the first statement of `build_index`; a test)
- Modify: `crates/contrib/pgvector_hnsw_build/Cargo.toml` (add `guc_seams`)
- Modify: `Cargo.lock`

**Interfaces:**
- Produces: `pub fn seed_random(seed: u64)` in `pgvector_hnsw::insert`. Also produces the SQL-visible placeholder `pgrust.hnsw_build_seed`: unset means no seeding, and an unsigned integer N means `SeedRandom(N)` at build start. Tasks 6 and 9 run `SET pgrust.hnsw_build_seed = 42`.

- [ ] **Step 1: Write the failing tests**

Add inside `mod tests` in `insert.rs`. The vectors come from C `pg_prng.c` built with PG 18.6 headers; the program is in `~/.cache/pgrust/m1-research/bytecmp/prng/`.
```rust
    // SeedRandom(42) then RandomDouble() (hnsw.h:105-106) against C pg_prng.c.
    #[test]
    fn seed_random_42_level_stream_matches_c() {
        seed_random(42);
        for want in [
            0x3fda7a16cd8c4e04u64, 0x3fcde1962a0eb130, 0x3fcf1aef3259d9b8, 0x3fb06e3c0092b080,
            0x3f8e650b88680600, 0x3fc5f3da196c34f8, 0x3fe54b86c216e79c, 0x3fc910659855e9e8,
        ] {
            assert_eq!(pg_prng::global_prng(|p| p.next_f64()).to_bits(), want);
        }
        let (ml, max) = (hnsw_get_ml(16), crate::layout::hnsw_get_max_level(16));
        seed_random(42);
        let lv: Vec<u8> = (0..16).map(|_| random_level(ml, max)).collect();
        assert_eq!(lv, [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
        seed_random(42);
        let mut h = [0u32; 4];
        for _ in 0..1000 {
            h[random_level(ml, max) as usize] += 1;
        }
        assert_eq!(h, [928, 68, 4, 0]);
    }
```
Add inside `mod tests` in `pgvector_hnsw_build/src/lib.rs`:
```rust
    #[test]
    fn parse_build_seed_accepts_only_unsigned_integers() {
        assert_eq!(parse_build_seed(None), None);
        assert_eq!(parse_build_seed(Some("42".to_string())), Some(42));
        assert_eq!(parse_build_seed(Some(" 7 ".to_string())), Some(7));
        assert_eq!(parse_build_seed(Some(String::new())), None);
        assert_eq!(parse_build_seed(Some("-1".to_string())), None);
        assert_eq!(parse_build_seed(Some("abc".to_string())), None);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `UT seed_random_42` and `UT parse_build_seed`
Expected: compile errors, because `seed_random` and `parse_build_seed` aren't defined.

- [ ] **Step 3: Implement**

In `insert.rs`, below `random_level`:
```rust
// SeedRandom(seed) (hnsw.h:106): pg_prng_seed(&pg_global_prng_state, seed).
pub fn seed_random(seed: u64) {
    pg_prng::global_prng(|p| p.seed(seed));
}
```
In `crates/contrib/pgvector_hnsw_build/Cargo.toml` `[dependencies]`, add:
```toml
guc_seams = { path = "../../backend/utils/misc/guc_seams" }
```
In `pgvector_hnsw_build/src/lib.rs`:
- Change line 9 to `use pgvector_hnsw::insert::{form_index_value, insert_tuple_on_disk, random_level, seed_random};`.
- Add above `fn build_index`:
  ```rust
  // pgrust.hnsw_build_seed: a placeholder customized option, deliberately NOT a
  // registered GUC (a pg_settings row would break byte-identical regression
  // outputs; same pattern as pgrust.gather_fair_stride). Set to N, a build calls
  // SeedRandom(N) where BuildIndex calls SeedRandom(42) under #ifdef HNSW_MEMORY
  // (hnswbuild.c:1134-1136). Unset (the default) is stock C, which never seeds.
  // DIVERGENCE (opt-in only): stock C has no such switch; the byte-identical and
  // iterative-scan tiers set 42 to match the -DHNSW_MEMORY reference build.
  fn hnsw_build_seed() -> Option<u64> {
      // Uninstalled seam (unit-test binaries without a guc boot): stock C.
      if !guc_seams::get_config_option_missing_ok::is_installed() {
          return None;
      }
      parse_build_seed(
          guc_seams::get_config_option_missing_ok::call("pgrust.hnsw_build_seed")
              .ok()
              .flatten(),
      )
  }

  fn parse_build_seed(value: Option<String>) -> Option<u64> {
      value.and_then(|v| v.trim().parse::<u64>().ok())
  }
  ```
- Make the first statement of `build_index` (before `let graph_ctx = ...`):
  ```rust
      // BuildIndex (hnswbuild.c:1134-1136): SeedRandom before InitBuildState.
      if let Some(seed) = hnsw_build_seed() {
          seed_random(seed);
      }
  ```
  `hnswbuildempty` goes through `build_index`, as C's `BuildIndex` does, so unlogged init forks are seeded too.

- [ ] **Step 4: Update the lock file and run the tests**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
```
Expected: one small hunk adding `guc_seams` to `pgvector_hnsw_build`'s dependency list.

Run: `UT seed_random_42` and `UT parse_build_seed`
Expected: both PASS.

- [ ] **Step 5: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git add crates/contrib/pgvector_hnsw/src/insert.rs crates/contrib/pgvector_hnsw_build/src/lib.rs crates/contrib/pgvector_hnsw_build/Cargo.toml Cargo.lock
git commit -q -F - <<'EOF'
pgvector_hnsw_build: opt-in pgrust.hnsw_build_seed (C's HNSW_MEMORY seed)

Stock pgvector never seeds HNSW builds; only a -DHNSW_MEMORY build calls
SeedRandom(42). The placeholder option reproduces that build on demand
for the byte-identical tier; unset, pgrust stays stock C.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```
The integration test for the seed is the byte-identical tier (Task 6).

---

### Task 6: Seeded C reference and the byte-identical HNSW tier

**Files:**
- Modify: `scripts/pgvector/common.sh`, `scripts/pgvector/build-reference.sh`, `scripts/pgvector/build-pgrust.sh`, `scripts/pgvector/run-all.sh`, `scripts/pgvector/tests/harness_test.sh`
- Create: `scripts/pgvector/run-bytecmp.sh`

**Interfaces:**
- Consumes: `pgrust.hnsw_build_seed` (Task 5).
- Produces, in `common.sh`: `PG_VEC_SEEDED` (default `$PGREF/pgvec-seeded`), `PGV_PORT_SEEDED=55494`, `PAGEMASK_BIN`, and the `seeded` mode for `port_for`, `assert_identity` and `server_start`.
- Produces `run-bytecmp.sh {pgrust|ref} [case ...] | --list`. It writes `$PGV_WORK/bytecmp/<mode>/summary.tsv` (`case<TAB>ok|FAIL`), `cases.tsv` (`case<TAB>file<TAB>blocks<TAB>entry_level`) and `diff/*.txt`.
  - Environment: `PGV_BYTECMP_SUBJECT={pgrust|ref|seeded}` overrides the subject.
  - `PGV_BYTECMP_SEED` is the seed `SET` on each side (default `42`; empty means no `SET`).
- Task 9 copies the run-side pattern for `run-iterscan.sh`.

- [ ] **Step 1: Write the failing self-tests**

In `scripts/pgvector/tests/harness_test.sh`, insert above the `# --- runner ---` line:
```bash
# Byte-identical tier, C vs C: two fresh seeded clusters must produce
# identical index files for every default case.
test_bytecmp_ref() {
  local n want
  want="$(set -- $("$here/../run-bytecmp.sh" --list) && echo $#)"
  if "$here/../run-bytecmp.sh" ref >/dev/null 2>&1; then
    pass "bytecmp ref: exit 0"
  else
    fail "bytecmp ref: exit 0 (see $PGV_WORK/bytecmp/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/bytecmp/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = "$want" ]; then pass "bytecmp ref: $want ok"; else fail "bytecmp ref: '$n' ok, want $want"; fi
  if assert_reference_clean; then pass "bytecmp: reference trees untouched"; else fail "bytecmp: reference trees modified"; fi
}

# The cases must exercise what they claim (reads the C-vs-C run above):
# multi-level graphs, a deep m=4 graph, page-split wide tuples, an init fork.
test_bytecmp_cases() {
  local f="$PGV_WORK/bytecmp/ref/cases.tsv" c label blocks level
  if [ ! -s "$f" ]; then
    fail "bytecmp cases: no $f (run test_bytecmp_ref first)"
    return
  fi
  while IFS=$'\t' read -r c label blocks level; do
    case "$c:$label" in
      *:idx_init) [ "$level" = -1 ] && pass "bytecmp $c: empty init fork" || fail "bytecmp $c: init fork entry level $level" ;;
      m4_golomb:idx) [ "$level" -ge 3 ] && pass "bytecmp $c: graph height $level" || fail "bytecmp $c: graph height $level, want >= 3" ;;
      wide_golomb:idx) [ "$blocks" -ge 150 ] && pass "bytecmp $c: $blocks blocks" || fail "bytecmp $c: $blocks blocks, want >= 150" ;;
      *) [ "$level" -ge 1 ] && [ "$blocks" -ge 10 ] && pass "bytecmp $c: $blocks blocks, height $level" ||
        fail "bytecmp $c: $blocks blocks, height $level (want multi-page, multi-level)" ;;
    esac
  done <"$f"
}

# Negative control: stock C pgvector never seeds (SeedRandom(42) is under
# #ifdef HNSW_MEMORY), so an unseeded subject must be reported as different.
test_bytecmp_detects_difference() {
  local w="$PGV_WORK/bytecmp-neg"
  if PGV_WORK="$w" PGV_BYTECMP_SUBJECT=ref "$here/../run-bytecmp.sh" ref l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp flags an unseeded C build"
  elif grep -q $'^l2_golomb\tFAIL$' "$w/bytecmp/ref/summary.tsv" 2>/dev/null; then
    pass "bytecmp flags an unseeded C build"
  else
    fail "bytecmp flags an unseeded C build: wrong failure (see $w.log)"
  fi
}

# Review focus 1: the seed is opt-in. Without SET pgrust.hnsw_build_seed,
# pgrust builds like stock C and so differs from the seeded oracle.
test_bytecmp_knob_off() {
  local w="$PGV_WORK/bytecmp-noseed"
  if PGV_WORK="$w" PGV_BYTECMP_SEED= "$here/../run-bytecmp.sh" pgrust l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp: unseeded pgrust differs from the seeded oracle"
  elif grep -q $'^l2_golomb\tFAIL$' "$w/bytecmp/pgrust/summary.tsv" 2>/dev/null; then
    pass "bytecmp: unseeded pgrust differs from the seeded oracle"
  else
    fail "bytecmp: unseeded pgrust: wrong failure (see $w.log)"
  fi
}

# The oracle must be the seeded build, and pgrust mode must refuse a C server.
test_bytecmp_identity() {
  local w="$PGV_WORK/bytecmp-ident"
  if PGV_WORK="$w" PG_VEC_SEEDED="$PG_VEC" "$here/../run-bytecmp.sh" ref l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp refuses an unseeded oracle"
  elif grep -q 'not built by the seeded C pgvector' "$w.log"; then
    pass "bytecmp refuses an unseeded oracle"
  else
    fail "bytecmp refuses an unseeded oracle: wrong error (see $w.log)"
  fi
  if PGV_WORK="$w" PGRUST_BIN="$PG_VEC/bin/postgres" "$here/../run-bytecmp.sh" pgrust l2_golomb >"$w.log" 2>&1; then
    fail "bytecmp pgrust mode refuses a C server"
  elif grep -q 'not a pgrust server' "$w.log"; then
    pass "bytecmp pgrust mode refuses a C server"
  else
    fail "bytecmp pgrust mode refuses a C server: wrong error (see $w.log)"
  fi
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/pgvector/tests/harness_test.sh test_bytecmp_ref`
Expected: `FAIL - bytecmp ref: exit 0` (`run-bytecmp.sh` does not exist), and a non-zero exit.

- [ ] **Step 3: Extend `common.sh`**

Make these edits to `scripts/pgvector/common.sh`:
- After the header comment's `$PG_VEC` paragraph, add:
  ```bash
  #   $PG_VEC_SEEDED  Same as $PG_VEC, but pgvector is built with -DHNSW_MEMORY,
  #              the only configuration in which C seeds the HNSW level RNG
  #              (SeedRandom(42), hnswbuild.c:1134-1136). Oracle of the
  #              byte-identical and iterative-scan tiers; nothing else uses it.
  ```
- After `PG_VEC="$PGREF/pgvec"`, add `PG_VEC_SEEDED="${PG_VEC_SEEDED:-$PGREF/pgvec-seeded}"`.
- After `PGV_PORT_DOCKER=55493`, add `PGV_PORT_SEEDED=55494`.
- After the `PGRUST_BIN=...` line, add `PAGEMASK_BIN="${PAGEMASK_BIN:-$PGV_REPO/target/$PGRUST_PROFILE/pagemask}"`.
- In `port_for`, add `    seeded) echo "$PGV_PORT_SEEDED" ;;` after the `ref)` arm.
- In `assert_identity`, add these two arms before the `*)` arm:
  ```bash
      "seeded:"*"(pgrust "*) die "port $port is a pgrust server, expected the seeded C reference: $v" ;;
      "seeded:PostgreSQL 18.6"*) ;;
  ```
- In `server_start`, add these lines right after `mkdir -p "$PGV_WORK/sock" "$PGV_WORK/log"`:
  ```bash
    # Unix-domain socket paths are limited to 103 bytes on macOS.
    [ "${#PGV_WORK}" -le 80 ] || die "PGV_WORK is too long for a socket path (${#PGV_WORK} > 80 bytes): $PGV_WORK"
  ```
- In `server_start`, add this arm after the `ref)` launch arm:
  ```bash
      seeded)
        [ -x "$PG_VEC_SEEDED/bin/postgres" ] || die "missing $PG_VEC_SEEDED; run scripts/pgvector/build-reference.sh"
        (exec "$PG_VEC_SEEDED/bin/postgres" -D "$data" -k "$PGV_WORK/sock" -p "$port" -c listen_addresses=) >"$log" 2>&1 &
        ;;
  ```

- [ ] **Step 4: Build the seeded install (`build-reference.sh` step 4)**

In `scripts/pgvector/build-reference.sh`, inside `verify_reference` before the final `[ "$fails" -eq 0 ] ...` line, add:
```bash
  # HNSW_MEMORY compiles in FlushPages' INFO "memory: %zu MB" (hnswbuild.c:306-308).
  hnsw_memory_marker() {
    local lib
    for lib in "$1/vector.dylib" "$1/vector.so"; do
      if [ -f "$lib" ]; then
        strings "$lib" | grep -c 'memory: %zu MB' || true
        return
      fi
    done
    echo missing
  }
  seeded_share="$("$PG_VEC_SEEDED/bin/pg_config" --sharedir 2>/dev/null || true)"
  expect "seeded postgres -V" "$("$PG_VEC_SEEDED/bin/postgres" -V 2>/dev/null)" "postgres (PostgreSQL) 18.6"
  expect "seeded reference has pgvector" \
    "$(sed -n "s/^default_version = '\(.*\)'$/\1/p" "$seeded_share/extension/vector.control" 2>/dev/null)" "0.8.7"
  expect "seeded pgvector built with HNSW_MEMORY" \
    "$(hnsw_memory_marker "$("$PG_VEC_SEEDED/bin/pg_config" --pkglibdir 2>/dev/null)")" "1"
  expect "reference pgvector built without HNSW_MEMORY" \
    "$(hnsw_memory_marker "$("$PG_VEC/bin/pg_config" --pkglibdir 2>/dev/null)")" "0"
```
Change `local fails=0 tools_share vec_share` to `local fails=0 tools_share vec_share seeded_share`.

Above the final `verify_reference` call, add:
```bash
# 4. Seeded reference install (oracle of run-bytecmp.sh and run-iterscan.sh):
#    the same pgvector built with -DHNSW_MEMORY, the only C configuration that
#    seeds HNSW builds (SeedRandom(42), hnswbuild.c:1134-1136). PG_CFLAGS must
#    come from the environment: on the make command line it would replace the
#    Makefile's `PG_CFLAGS += ... -ffp-contract=fast` instead of extending it.
if [ ! -d "$PG_VEC_SEEDED" ]; then
  cp -R "$PG_TOOLS" "$PG_VEC_SEEDED"
fi
seeded_share="$("$PG_VEC_SEEDED/bin/pg_config" --sharedir)"
if [ ! -f "$seeded_share/extension/vector.control" ]; then
  build="$PGREF/build/pgvector-seeded"
  rm -rf "$build"
  mkdir -p "$build"
  cp -R "$PGV_SRC/." "$build/"
  echo "building seeded pgvector 0.8.7 (-DHNSW_MEMORY) in $build"
  (
    cd "$build" &&
      PG_CFLAGS="-DHNSW_MEMORY" make OPTFLAGS="" PG_CONFIG="$PG_VEC_SEEDED/bin/pg_config" >make.log 2>&1 &&
      make install PG_CONFIG="$PG_VEC_SEEDED/bin/pg_config" >install.log 2>&1
  ) || die "seeded pgvector build failed; logs in $build"
fi
```
Run:
```bash
scripts/pgvector/build-reference.sh
```
Expected: every check is `ok`, including the four new ones.

- [ ] **Step 5: Build `pagemask` with the server**

In `scripts/pgvector/build-pgrust.sh`, change the cargo line so both binaries share the server build's feature resolution:
```bash
(cd "$PGV_REPO" && cargo build --profile "$PGRUST_PROFILE" --locked --bin postgres --bin pagemask)
```
Then add this line after the `-V` check:
```bash
[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN after the build"
```
Run `scripts/pgvector/build-pgrust.sh`. Expected: `built .../postgres (...)`.

- [ ] **Step 6: Write `run-bytecmp.sh`**

Create `scripts/pgvector/run-bytecmp.sh`:
```bash
#!/usr/bin/env bash
# Byte-identical HNSW tier (spec §8.2). Build the same HNSW indexes on the
# seeded C reference (the oracle) and on the subject server, stop both cleanly,
# then compare every index relation file page by page after masking both sides
# with crates/bin/pagemask's `generic` rmgr: pd_lsn, pd_checksum and the
# pd_lower..pd_upper gap (generic_mask, generic_xlog.c:539; HNSW pages are
# written through GenericXLog). Everything else on every page must match.
#
# usage: run-bytecmp.sh {pgrust|ref} [case ...]   (default: the cases in CASES)
#        run-bytecmp.sh --list
#   pgrust: the subject is pgrust ($PGRUST_BIN), seeded with
#           SET pgrust.hnsw_build_seed = $PGV_BYTECMP_SEED (default 42).
#   ref:    the subject is a second, fresh seeded C cluster: C-vs-C proof that
#           the oracle and every case are deterministic (must always pass).
#   PGV_BYTECMP_SUBJECT={pgrust|ref|seeded} overrides the subject (self-tests);
#   PGV_BYTECMP_SEED= (empty) omits the SET (self-test of the opt-in seed).
#
# The oracle is $PG_VEC_SEEDED (see common.sh): stock C pgvector does not seed
# its level RNG at all (SeedRandom(42) sits under #ifdef HNSW_MEMORY), so only
# that build is deterministic. Data are integer-valued so float math is exact
# or rounds once (identical under FMA and reassociation). Every case runs in its
# own session; insert_* cases build and then insert in ONE session, because
# on-disk inserts draw levels from the stream the build left behind.
set -euo pipefail
. "$(dirname "$0")/common.sh"

# Default cases. *_golomb data are tie-free: x = 2*p*k + (k*k % p) is an
# Erdos-Turan Golomb ruler, so all pairwise distances differ. *_grid and dups
# have many equal distances, which exercises C's pairingheap tie order.
CASES="l2_golomb l1_golomb m4_golomb wide_golomb unlogged_golomb insert_golomb l2_grid ip_grid dups insert_grid"
# Opt-in only (known divergences, run by name): spill cosine

GOLOMB1009='ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector'
GRID='ARRAY[i % 17, (i * 7) % 23, (i * 13) % 31]::vector'
SEED="${PGV_BYTECMP_SEED-42}"

# Print one case's SQL. Each case ends by selecting "rel|<label>|<relfile>"
# rows for the files to compare (paths relative to the data directory).
case_sql() {
  echo "SET max_parallel_maintenance_workers = 0;"
  echo "SET maintenance_work_mem = '64MB';"
  # Seeds pgrust (Task 5); C keeps it as an unused placeholder.
  [ -z "$SEED" ] || echo "SET pgrust.hnsw_build_seed = $SEED;"
  case "$1" in
    l2_golomb | l1_golomb)
      local ops=vector_l2_ops
      [ "$1" = l1_golomb ] && ops=vector_l1_ops
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
SQL
      ;;
    m4_golomb)
      # m = 4: about one element in four gets level >= 1 (levels reach 5).
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, ARRAY[2*2003*k + (k*k) % 2003, 0, 0]::vector FROM (SELECT (i * 37) % 2003 AS k FROM generate_series(0, 1999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops) WITH (m = 4, ef_construction = 16);
SQL
      ;;
    wide_golomb)
      # 2000 dimensions: element + neighbor tuple exceed HNSW_MAX_SIZE, so the
      # neighbor tuple goes on the next page (CreateGraphPages).
      cat <<SQL
CREATE TABLE $1 (id int, v vector(2000));
INSERT INTO $1 SELECT k, (ARRAY[2*1009*k + (k*k) % 1009] || array_fill(0, ARRAY[1999]))::vector FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 99) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    unlogged_golomb)
      # Also compares the init fork written by hnswbuildempty.
      cat <<SQL
CREATE UNLOGGED TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SELECT 'rel|idx_init|' || pg_relation_filepath('$1_idx') || '_init';
SQL
      ;;
    insert_golomb)
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(0, 799) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
INSERT INTO $1 SELECT k, $GOLOMB1009 FROM (SELECT (i * 37) % 1009 AS k FROM generate_series(800, 999) i) s;
SQL
      ;;
    l2_grid | ip_grid)
      local ops=vector_l2_ops
      [ "$1" = ip_grid ] && ops=vector_ip_ops
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v $ops);
SQL
      ;;
    dups)
      # Every value three times: heaptids arrays (FindDuplicateInMemory).
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GOLOMB1009 FROM (SELECT i, ((i / 3) * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    insert_grid)
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 800) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
INSERT INTO $1 SELECT i, $GRID FROM generate_series(801, 1000) i;
SQL
      ;;
    spill)
      # Known divergence: C flushes when its Generation context has allocated
      # maintenance_work_mem (1436 tuples here); pgrust estimates (1840).
      cat <<SQL
SET maintenance_work_mem = '1MB';
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, $GRID FROM generate_series(1, 6000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_l2_ops);
SQL
      ;;
    cosine)
      # Known divergence: normalized values are not integers, and C's
      # -ffp-contract=fast fuses multiply-adds that pgrust rounds twice.
      cat <<SQL
CREATE TABLE $1 (id int, v vector(3));
INSERT INTO $1 SELECT i, ARRAY[1 + i % 17, 1 + (i * 7) % 23, 1 + (i * 13) % 31]::vector FROM generate_series(1, 1000) i;
CREATE INDEX $1_idx ON $1 USING hnsw (v vector_cosine_ops);
SQL
      ;;
    *) die "unknown case '$1' (default cases: $CASES; opt-in: spill cosine)" ;;
  esac
  echo "SELECT 'rel|idx|' || pg_relation_filepath('$1_idx');"
}

# run_side LABEL KIND: fresh cluster of server KIND, run every case, stop
# cleanly (the shutdown checkpoint writes every buffer, unlogged ones too),
# then copy and mask the files into $out/LABEL.
run_side() {
  local label="$1" kind="$2" port dir c line rel path
  port="$(port_for "$kind")"
  dir="$out/$label"
  mkdir -p "$dir"
  server_stop "$kind"
  rm -rf "$out/data/$label"
  server_start "$kind" "$out/data/$label" "$port"
  pgv_psql "$port" -c 'CREATE EXTENSION vector'
  for c in "${cases[@]}"; do
    case_sql "$c" >"$dir/$c.sql"
    pgv_psql "$port" -At -f "$dir/$c.sql" >"$dir/$c.out" 2>"$dir/$c.err" ||
      die "$label ($kind): case $c failed; see $dir/$c.err"
    # The seeded build reports its graph memory (HNSW_MEMORY's FlushPages
    # elog), which proves the seeded library built the index.
    if [ "$kind" = seeded ] && ! grep -q 'INFO:  memory: ' "$dir/$c.err"; then
      die "$label: case $c was not built by the seeded C pgvector (no 'INFO:  memory:' line)"
    fi
  done
  server_stop "$kind"
  cp "$PGV_WORK/log/server-$kind.log" "$dir/server.log"
  for c in "${cases[@]}"; do
    while IFS= read -r line; do
      case "$line" in
        rel\|*) ;;
        *) continue ;;
      esac
      rel="${line#rel|}"
      path="${rel#*|}"
      rel="${rel%%|*}"
      cp "$out/data/$label/$path" "$dir/$c.$rel"
      "$PAGEMASK_BIN" generic "$dir/$c.$rel" "$dir/$c.$rel.masked" ||
        die "pagemask failed on $dir/$c.$rel"
    done <"$dir/$c.out"
  done
}

# HnswMetaPageData.entryLevel (int16 at page offset 24 + 22): the graph height.
entry_level() {
  od -An -td2 -j46 -N2 "$1" | tr -d ' '
}

# Per-block summary of the masked bytes that differ.
describe_diff() {
  local a="$1" b="$2"
  echo "oracle $(wc -c <"$a" | tr -d ' ') bytes, subject $(wc -c <"$b" | tr -d ' ') bytes"
  { cmp -l "$a" "$b" 2>&1 || true; } | awk -v bs=8192 '
    /EOF/ { print; next }
    { o = $1 - 1; blk = int(o / bs); n[blk]++; if (!(blk in first)) first[blk] = o % bs; total++ }
    END {
      printf "%d byte(s) differ after masking\n", total
      for (blk in n) printf "block %d: %d byte(s), first at page offset %d\n", blk, n[blk], first[blk]
    }' | sort -n -k2 | head -40
}

if [ "${1:-}" = --list ]; then
  echo "$CASES"
  exit 0
fi
[ "$#" -ge 1 ] || die "usage: run-bytecmp.sh {pgrust|ref} [case ...]"
mode="$1"
shift
case "$mode" in
  pgrust) subject="${PGV_BYTECMP_SUBJECT:-pgrust}" ;;
  ref) subject="${PGV_BYTECMP_SUBJECT:-seeded}" ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
if [ "$#" -gt 0 ]; then
  cases=("$@")
else
  # shellcheck disable=SC2206 # intentional splitting: case names have no spaces
  cases=($CASES)
fi
for c in "${cases[@]}"; do case_sql "$c" >/dev/null; done

[ -x "$PAGEMASK_BIN" ] || die "missing $PAGEMASK_BIN; run scripts/pgvector/build-pgrust.sh"
out="$PGV_WORK/bytecmp/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
trap 'server_stop seeded; server_stop "$subject"' EXIT

run_side oracle seeded
run_side subject "$subject"

: >"$out/summary.tsv"
: >"$out/cases.tsv"
for c in "${cases[@]}"; do
  result=ok
  for f in "$out/oracle/$c".*.masked; do
    [ -e "$f" ] || die "case $c produced no relation files"
    name="$(basename "$f" .masked)"
    g="$out/subject/$name.masked"
    printf '%s\t%s\t%s\t%s\n' "$c" "${name#"$c".}" "$(($(wc -c <"$f") / 8192))" \
      "$(entry_level "$f")" >>"$out/cases.tsv"
    if [ ! -e "$g" ] || ! cmp -s "$f" "$g"; then
      result=FAIL
      if [ -e "$g" ]; then describe_diff "$f" "$g"; else echo "subject has no $name"; fi >"$out/diff/$name.txt"
    fi
  done
  printf '%s\t%s\n' "$c" "$result" | tee -a "$out/summary.tsv"
done

passed="$(grep -c $'\tok$' "$out/summary.tsv" || true)"
echo "bytecmp ($mode, subject $subject): $passed/${#cases[@]} identical; diffs in $out/diff"
[ "$passed" -eq "${#cases[@]}" ]
```
Then make it executable: `chmod +x scripts/pgvector/run-bytecmp.sh`.

- [ ] **Step 7: Run C vs C, then the self-tests**

```bash
scripts/pgvector/run-bytecmp.sh ref
scripts/pgvector/tests/harness_test.sh test_bytecmp_ref test_bytecmp_cases test_bytecmp_detects_difference test_bytecmp_identity test_bytecmp_knob_off
```
Expected:
- `bytecmp (ref, subject seeded): 10/10 identical`, in about 10 seconds.
- The self-tests print `all passed`. `test_bytecmp_knob_off` uses pgrust, so it depends on Steps 3–5 and Task 5.

- [ ] **Step 8: Run pgrust and record the expected tie failures**

```bash
scripts/pgvector/run-bytecmp.sh pgrust || true
```
Expected:
- The 6 `*_golomb` cases are `ok`: the seed is the only divergence on tie-free data.
- `l2_grid`, `ip_grid`, `dups` and `insert_grid` are `FAIL`, with differences only in neighbor tuples. Pairing-heap tie order is still missing; Task 7 adds it.
- If a `*_golomb` case FAILs, stop and investigate with `~/.cache/pgrust/m1-research/bytecmp/hnswdiff.py <oracle file> <subject file>` before going on.

- [ ] **Step 9: Add the tier to `run-all.sh`**

In `scripts/pgvector/run-all.sh`:
- After `"$here/run-tap.sh" "$mode" || true`, add `"$here/run-bytecmp.sh" "$mode" || true`.
- Change `for tier in regress tap; do` to `for tier in regress tap bytecmp; do`.

- [ ] **Step 10: Commit**

```bash
git add scripts/pgvector/{common.sh,build-reference.sh,build-pgrust.sh,run-bytecmp.sh,run-all.sh} scripts/pgvector/tests/harness_test.sh
git commit -q -F - <<'EOF'
pgvector harness: seeded C reference and the byte-identical HNSW tier

pgvec-seeded is pgvector built with -DHNSW_MEMORY, the only C build that
seeds HNSW levels. run-bytecmp.sh builds 10 cases on it and on the
subject and compares pagemask-masked index files; C vs C is 10/10.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 7: C's pairing-heap tie order in the build, insert and scan searches

**Files:**
- Modify: `crates/_support/types/types_hnsw/Cargo.toml` (add `pairingheap`), `crates/_support/types/types_hnsw/src/lib.rs` (`ScanDiscardedHeap` replaces `DistanceMinHeap`)
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs`:
  - add the candidate comparators and heaps, and `ElementPool::scan_element`;
  - `search_layer_disk` (lines 641-751) gets pairing heaps;
  - delete `DiscardedHeap`, `MinHeapF` and `MaxHeapF` (lines 485-639).
- Modify: `crates/contrib/pgvector_hnsw/src/scan.rs` (discarded handling; delete `pool_elem_to_scan`)
- Modify: `crates/contrib/pgvector_hnsw_build/src/lib.rs:95-189` (`search_layer_mem`)
- Modify: `Cargo.lock`

**Interfaces:**
- Produces in `types_hnsw`:
  - `pub type ScanDiscardedHeap = pairingheap::PairingHeap<HnswScanElement, fn(&HnswScanElement, &HnswScanElement) -> i32>`
  - `pub fn compare_nearest_discarded(a, b) -> i32`
  - `pub fn new_scan_discarded_heap() -> ScanDiscardedHeap`
  - `HnswScanOpaqueData.discarded: Option<ScanDiscardedHeap>`
- Produces in `pgvector_hnsw::utils`:
  - `pub type CandHeap = PairingHeap<(f64, u32), fn(&(f64, u32), &(f64, u32)) -> i32>`
  - `pub fn compare_nearest_candidates(..) -> i32` and `pub fn compare_furthest_candidates(..) -> i32`
  - `pub fn nearest_candidate_heap() -> CandHeap` and `pub fn furthest_candidate_heap() -> CandHeap`
  - `ElementPool::scan_element(&self, id: u32, distance: f64) -> HnswScanElement`
  - `search_layer_disk(.., visited: &mut Visited<'_>, discarded: Option<&mut ScanDiscardedHeap>, init_visited: bool, tuples: Option<&mut i64>)`. Task 9 changes the `visited` and `discarded` parameters and appends `tmp_ctx`.

- [ ] **Step 1: Write the failing comparator test**

Add to `mod tests` in `utils.rs` (created in Task 4):
```rust
    // pairingheap_first is the comparator's maximum: C pops the nearest
    // candidate (CompareNearestCandidates, hnswutils.c:632) and W the furthest
    // (CompareFurthestCandidates, hnswutils.c:662).
    #[test]
    fn candidate_heaps_pop_in_c_order() {
        let mut c = nearest_candidate_heap();
        let mut w = furthest_candidate_heap();
        for (d, e) in [(2.0, 1u32), (1.0, 2), (3.0, 3)] {
            c.add((d, e));
            w.add((d, e));
        }
        assert_eq!(c.remove_first(), Some((1.0, 2)));
        assert_eq!(w.remove_first(), Some((3.0, 3)));
        assert_eq!(w.first(), Some(&(2.0, 1)));
    }
```
Run: `UT candidate_heaps_pop_in_c_order`
Expected: a compile error, `cannot find function 'nearest_candidate_heap'`. The integration test that's actually failing is the four tie cases of `run-bytecmp.sh pgrust` (Task 6, Step 8).

- [ ] **Step 2: `types_hnsw`: the discarded heap type**

In `crates/_support/types/types_hnsw/Cargo.toml` `[dependencies]`, add `pairingheap = { path = "../../../backend/lib/pairingheap" }`.

In `crates/_support/types/types_hnsw/src/lib.rs`, replace the whole `DistanceMinHeap` block (its comment, struct, `Default` impl and `impl`) with:
```rust
// so->discarded (hnswscan.c; allocated at InitVisited, hnswutils.c:851-852):
// a pairingheap ordered by CompareNearestDiscardedCandidates. It holds
// self-contained copies of the discarded elements so it outlives each call's
// element pool, and it is filled in the exact order C adds to it, so ties pop
// in C's order. Owned (global allocator): hnswrescan drops it, as C's
// MemoryContextReset(so->tmpCtx) frees it.
pub type ScanDiscardedHeap =
    pairingheap::PairingHeap<HnswScanElement, fn(&HnswScanElement, &HnswScanElement) -> i32>;

// CompareNearestDiscardedCandidates (hnswutils.c:647-658): nearest first.
pub fn compare_nearest_discarded(a: &HnswScanElement, b: &HnswScanElement) -> i32 {
    if a.distance < b.distance {
        1
    } else if a.distance > b.distance {
        -1
    } else {
        0
    }
}

pub fn new_scan_discarded_heap() -> ScanDiscardedHeap {
    ScanDiscardedHeap::new(compare_nearest_discarded)
}
```
Change the field to `pub discarded: Option<ScanDiscardedHeap>,`.

- [ ] **Step 3: `utils.rs`: comparators, heaps and `scan_element`; switch `search_layer_disk` over**

At the top of `utils.rs`, add `use pairingheap::PairingHeap;`.

Delete `pub struct DiscardedHeap` with its `impl`, and `MinHeapF`/`MaxHeapF` with their `impl`s (lines 485-639). Put this in their place:
```rust
// Search heaps over (distance, element). C's pairingheap_first is the
// comparator's maximum; the pairingheap crate reproduces C's merge order, so
// equal distances pop in C's order (which decides neighbor lists and the
// order of tied scan results).
pub type CandHeap = PairingHeap<(f64, u32), fn(&(f64, u32), &(f64, u32)) -> i32>;

// CompareNearestCandidates (hnswutils.c:632-643): the nearest candidate first.
pub fn compare_nearest_candidates(a: &(f64, u32), b: &(f64, u32)) -> i32 {
    if a.0 < b.0 {
        1
    } else if a.0 > b.0 {
        -1
    } else {
        0
    }
}

// CompareFurthestCandidates (hnswutils.c:662-673): the furthest candidate first.
pub fn compare_furthest_candidates(a: &(f64, u32), b: &(f64, u32)) -> i32 {
    if a.0 < b.0 {
        -1
    } else if a.0 > b.0 {
        1
    } else {
        0
    }
}

// pairingheap_allocate(CompareNearestCandidates) (hnswutils.c:831).
pub fn nearest_candidate_heap() -> CandHeap {
    CandHeap::new(compare_nearest_candidates)
}

// pairingheap_allocate(CompareFurthestCandidates) (hnswutils.c:832).
pub fn furthest_candidate_heap() -> CandHeap {
    CandHeap::new(compare_furthest_candidates)
}
```
In `impl ElementPool`, add:
```rust
    // A self-contained copy of a loaded element for the scan's w list and
    // discarded heap (C keeps HnswElement pointers into so->tmpCtx).
    pub fn scan_element(&self, id: u32, distance: f64) -> HnswScanElement {
        let e = self.get(id);
        HnswScanElement {
            blkno: e.blkno,
            offno: e.offno,
            level: e.level,
            version: e.version,
            heaptids: e.heaptids,
            heaptids_len: e.heaptids_len,
            neighbor_page: e.neighbor_page,
            neighbor_offno: e.neighbor_offno,
            distance,
        }
    }
```
In `search_layer_disk`, make these changes and nothing else:
- the parameter `mut discarded: Option<&mut DiscardedHeap<'t>>` becomes `mut discarded: Option<&mut ScanDiscardedHeap>`;
- `let mut c_heap = MinHeapF::new();` becomes `let mut c_heap = nearest_candidate_heap();`;
- `let mut w_heap = MaxHeapF::new();` becomes `let mut w_heap = furthest_candidate_heap();`;
- every `c_heap.push(d, x)` and `w_heap.push(d, x)` becomes `c_heap.add((d, x))` and `w_heap.add((d, x))`;
- `c_heap.pop()` becomes `c_heap.remove_first()`;
- `w_heap.first().expect(..)` becomes `*w_heap.first().expect(..)`;
- `w_heap.pop()` becomes `w_heap.remove_first()`;
- both discarded pushes, `dh.push(SearchCandidate { element: X, distance: D })`, become `dh.add(pool.scan_element(X, D))`;
- the result loop becomes:
  ```rust
      let mut w: PgVec<'t, SearchCandidate> = mcx::vec_with_capacity_in_infallible(pool.mcx, 0);
      while let Some((d, x)) = w_heap.remove_first() {
          w.push(SearchCandidate { element: x, distance: d });
      }
      Ok(w)
  ```

- [ ] **Step 4: `scan.rs`: the persistent heap is filled directly**

- Delete `fn pool_elem_to_scan`, and replace each `pool_elem_to_scan(&pool, sc)` with `pool.scan_element(sc.element, sc.distance)`.
- In `get_scan_items`, replace `let mut discarded = iterative.then(|| DiscardedHeap::new(tmcx));` with:
  ```rust
      // GetScanItems passes &so->discarded only with iterative scans (hnswscan.c:63).
      so.discarded = iterative.then(new_scan_discarded_heap);
  ```
  Pass `so.discarded.as_mut()` instead of `discarded.as_mut()` to the layer-0 `search_layer_disk`, and delete the `if let Some(dh) = discarded { ... so.discarded = Some(out); }` block.
- In `resume_scan_items`:
  - change `let Some(e) = dh.pop() else { break };` to `let Some(e) = dh.remove_first() else { break };`;
  - delete `let mut discarded = DiscardedHeap::new(tmcx);`;
  - pass `so.discarded.as_mut()` instead of `Some(&mut discarded)`;
  - delete the final `let out = so.discarded...; for sc in discarded.items.iter() { out.push(...) }` loop.
- In `hnswgettuple`'s `drain_one` branch, change `.pop().expect("nonempty")` to `.remove_first().expect("nonempty")`.

- [ ] **Step 5: `pgvector_hnsw_build`: the in-memory search**

In `search_layer_mem` (`lib.rs:95-189`):
- delete the nested `push_min`/`pop_min`/`push_max`/`pop_max`/`heap_pop` functions;
- `let mut c_heap: Vec<(f64, u32)> = Vec::new();` becomes `let mut c_heap = nearest_candidate_heap();`;
- `let mut w_heap: Vec<(f64, u32)> = Vec::new();` becomes `let mut w_heap = furthest_candidate_heap();`;
- `push_min(&mut c_heap, x)` and `push_max(&mut w_heap, x)` become `c_heap.add(x)` and `w_heap.add(x)`;
- `pop_min(&mut c_heap)` becomes `c_heap.remove_first()`;
- `pop_max(&mut w_heap)` becomes `w_heap.remove_first()`;
- `*w_heap.first().expect(..)` keeps its `*`;
- `Vec::with_capacity(w_heap.len())` becomes `Vec::new()`.

`CandHeap` and its constructors come in through the existing `use pgvector_hnsw::utils::*;`.

- [ ] **Step 6: Lock file, unit tests, then all 10 bytecmp cases**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
```
Expected: `pairingheap` is added to `types_hnsw`'s dependencies.

Run: `UT candidate_heaps_pop_in_c_order` and `UT update_connection`. The latter is the existing build-crate tests. Expected: all pass.
```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-bytecmp.sh pgrust
```
Expected: `bytecmp (pgrust, subject pgrust): 10/10 identical`.

- [ ] **Step 7: Regression and TAP check**

```bash
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
scripts/pgvector/run-tap.sh pgrust 010_hnsw_wal.pl 011_hnsw_vacuum.pl 013_hnsw_vector_insert_recall.pl 014_hnsw_vector_vacuum_recall.pl 015_hnsw_vector_duplicates.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 046_hnsw_vacuum_scan.pl 047_hnsw_vacuum_insert.pl
```
Expected: all `ok`.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
git add crates/_support/types/types_hnsw crates/contrib/pgvector_hnsw/src/{utils,scan}.rs crates/contrib/pgvector_hnsw_build/src/lib.rs Cargo.lock
git commit -q -F - <<'EOF'
pgvector_hnsw: C pairing-heap tie order for HNSW searches

Binary heaps popped equal-distance candidates in a different order than
C's pairingheaps, so graphs built on tied data differed. Build, insert
and scan now use the C-exact pairingheap crate with C's comparators, and
the scan's discarded heap is filled in C's order. bytecmp: 10/10.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 8: The scan memory model: `AllocSetModel`, `ListShape`, `TidHash`

**Files:**
- Create: `crates/_support/types/types_hnsw/src/tmpctx.rs`
- Create: `crates/_support/types/types_hnsw/tests/data/hnswscan-tmpctx-043.trace`, `.../tests/data/hnswscan-tmpctx-trace.patch` and `.../tests/data/README.md`
- Modify: `crates/_support/types/types_hnsw/src/lib.rs` (add `pub mod tmpctx; pub use tmpctx::*;`) and `crates/_support/types/types_hnsw/Cargo.toml` (add `hashfn`)
- Modify: `Cargo.lock`

**Interfaces:**
- Produces in `types_hnsw` (re-exported at the crate root). Task 9 uses these:
  - `HNSW_ELEMENT_DATA_SIZE = 128`, `HNSW_SEARCH_CANDIDATE_SIZE = 64`, `PAIRINGHEAP_SIZE = 24` and `HNSW_UNVISITED_SIZE = 8`, all `usize`.
  - `AllocSetModel::new(min_context_size, init_block_size, max_block_size)`, with `reset()`, `mem_allocated() -> usize`, `palloc(size)`, `pfree(size)` and `repalloc(old, new)`.
  - `ListShape`: `ListShape::NIL`, `len()`, `lappend(&mut AllocSetModel)`, `ListShape::lappend_n(n, &mut AllocSetModel) -> ListShape` and `delete_last(&mut AllocSetModel)`.
  - `TidHash::create(nelements: u32, Option<&mut AllocSetModel>) -> TidHash`, `insert(blkno: u32, offno: u16, Option<&mut AllocSetModel>) -> bool` (where `bool` means found), `size() -> u64` and `members() -> u32`.

The code was validated in a scratch crate (`~/.cache/pgrust/m1-research/iterscan/model/`) against an instrumented C build of pgvector 0.8.7. It matched C's `MemoryContextMemAllocated(so->tmpCtx)` at 39,580 checkpoints, and C's `tidhash` found/size results over 2,787,973 inserts.

- [ ] **Step 1: Put the C trace and its provenance into the repo**

```bash
D=crates/_support/types/types_hnsw/tests/data
mkdir -p "$D"
head -n 99852 ~/.cache/pgrust/m1-research/iterscan/trace-043-mult1-allocs.txt >"$D/hnswscan-tmpctx-043.trace"
grep -v '^Binary files ' ~/.cache/pgrust/m1-research/iterscan/tmpctx-shadow-instrumentation.patch >"$D/hnswscan-tmpctx-trace.patch"
tail -1 "$D/hnswscan-tmpctx-043.trace"; wc -c <"$D/hnswscan-tmpctx-043.trace"; grep -c '^C ' "$D/hnswscan-tmpctx-043.trace"
```
Expected: the last line starts with `C `, the file is 556,057 bytes, and there are 523 checkpoints.

Create `crates/_support/types/types_hnsw/tests/data/README.md`:
```markdown
# hnswscan tmpCtx trace

`hnswscan-tmpctx-043.trace` is the first 99,852 events of an allocation trace
recorded from C pgvector 0.8.7 with `hnswscan-tmpctx-trace.patch` applied
(`patch -p1` in a copy of `crates/pgvector-0.8.7-reference`; build with
`PG_CFLAGS=-DHNSW_MEMORY make OPTFLAGS=""`; run with
`HNSW_SHADOW_LOG=<file>`), on TAP 043's first query with
`hnsw.scan_mem_multiplier = 1`.

Events, one per line, in so->tmpCtx only:

- `B` scan start
- `R` MemoryContextReset
- `A <size>` palloc
- `F <size>` pfree
- `H <nelements> <buckets>` tidhash_create
- `C <bytes> <tuples>` checkpoint: C's real `MemoryContextMemAllocated(so->tmpCtx, false)`

`tmpctx.rs`'s `replay_c_trace` feeds A/F/R into `AllocSetModel` and requires
its byte count to equal C's at every checkpoint, and `TidHash::create` to pick
C's bucket count at every `H`.
```

- [ ] **Step 2: Write `tmpctx.rs` with its tests first**

Create `crates/_support/types/types_hnsw/src/tmpctx.rs` with only the test module, so the tests fail before the code exists:
```rust
//! Byte-exact model of hnswscan.c's so->tmpCtx (implementation in Step 4).

#[cfg(test)]
mod tests {
    use super::*;

    // Event shape seen in the C trace at the end of every ef=40 layer-0
    // search: A 64, A 128, A 256, F 128, A 512, F 256; and on emptying:
    // F 512, F 64.
    #[test]
    fn list_of_40_matches_c_trace_shape() {
        let mut a = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        let mut b = a.clone();
        let mut l = ListShape::lappend_n(40, &mut a);
        for s in [64usize, 128, 256] {
            b.palloc(s);
        }
        b.pfree(128);
        b.palloc(512);
        b.pfree(256);
        assert_eq!(a.mem_allocated(), b.mem_allocated());
        assert_eq!(a.nfree, b.nfree);
        for _ in 0..40 {
            l.delete_last(&mut a);
        }
        b.pfree(512);
        b.pfree(64);
        assert_eq!(a.nfree, b.nfree);
        assert_eq!(l, ListShape::NIL);
    }

    #[test]
    fn keeper_block_holds_58_elements_then_8k_block() {
        // 8192 - 200 (context) - 40 (block hdr) = 7952 = 58 * (128 + 8) + 64.
        let mut m = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        for _ in 0..58 {
            m.palloc(HNSW_ELEMENT_DATA_SIZE);
        }
        assert_eq!(m.mem_allocated(), 8192);
        m.palloc(HNSW_ELEMENT_DATA_SIZE);
        assert_eq!(m.mem_allocated(), 8192 + 8192);
        // The 64-byte tail was carved into a 32- and a 16-byte free chunk.
        assert_eq!(m.nfree[2], 1);
        assert_eq!(m.nfree[1], 1);
        // tidhash for ef_search=40, m=16: the 48-byte header comes from the
        // second block's free space; the 2048 buckets (16384 B) exceed the
        // 8192-byte chunk limit, so they get a dedicated block of
        // 16384 + 40 (block header) + 8 (chunk header).
        let t = TidHash::create(40 * 16 * 2, Some(&mut m));
        assert_eq!(t.size(), 2048);
        assert_eq!(m.mem_allocated(), 8192 + 8192 + 16384 + 48);
    }

    #[test]
    fn fresh_scan_context_is_one_keeper_block() {
        let m = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        assert_eq!(m.mem_allocated(), 8192);
    }

    #[test]
    fn tidhash_set_semantics() {
        assert_eq!(TidHash::create(32, None).size(), 64);
        let mut t = TidHash::create(32, None);
        assert!(!t.insert(7, 3, None));
        assert!(t.insert(7, 3, None));
        assert!(!t.insert(7, 4, None));
        for blk in 0..10_000u32 {
            t.insert(blk, (blk % 300) as u16 + 1, None);
        }
        for blk in 0..10_000u32 {
            assert!(t.insert(blk, (blk % 300) as u16 + 1, None));
        }
        assert!(t.members() >= 10_000);
        assert!(t.size().is_power_of_two() && t.size() as f64 * 0.9 >= t.members() as f64);
    }

    // Replay C's real allocation sequence (tests/data/README.md): the model
    // must equal C's MemoryContextMemAllocated at every checkpoint.
    #[test]
    fn replay_c_trace() {
        let trace = include_str!("../tests/data/hnswscan-tmpctx-043.trace");
        let mut m = AllocSetModel::new(0, 8 * 1024, 256 * 1024);
        let mut checks = 0u32;
        for line in trace.lines() {
            // Skip the event letter; `num` yields the event's numbers in order.
            let mut it = line.split_whitespace().skip(1);
            let mut num = || it.next().unwrap().parse::<u64>().unwrap();
            match line.as_bytes()[0] {
                b'B' => {}
                b'R' => m.reset(),
                b'A' => m.palloc(num() as usize),
                b'F' => m.pfree(num() as usize),
                b'H' => {
                    let n = num() as u32;
                    let size = num();
                    assert_eq!(TidHash::create(n, None).size(), size, "tidhash_create({n})");
                }
                b'C' => {
                    assert_eq!(m.mem_allocated() as u64, num(), "checkpoint {checks}: {line}");
                    checks += 1;
                }
                _ => panic!("bad event {line}"),
            }
        }
        assert_eq!(checks, 523);
    }
}
```

In `crates/_support/types/types_hnsw/src/lib.rs`, after the crate doc comment, add:
```rust
pub mod tmpctx;
pub use tmpctx::*;
```

- [ ] **Step 3: Run the tests to see them fail**

Run: `UT tmpctx`
Expected: compile errors, because `AllocSetModel`, `ListShape`, `TidHash` and the size constants aren't defined.

- [ ] **Step 4: Implement**

In `crates/_support/types/types_hnsw/Cargo.toml` `[dependencies]`, add `hashfn = { path = "../../../common/hashfn" }`. Then replace the first line of `tmpctx.rs` (keep the test module) with:
```rust
//! Byte-exact model of hnswscan.c's so->tmpCtx, the AllocSet whose
//! MemoryContextMemAllocated the iterative scan compares against
//! work_mem * hnsw.scan_mem_multiplier (hnswscan.c:264). DIVERGENCE: pgrust
//! does not palloc these structures in a C AllocSet; the scan feeds this model
//! C's allocation sequence (the sizes below, at C's allocation points) so the
//! cap trips at C's tuple. Validated against an instrumented C build; see
//! tests/data/README.md.

// C struct sizes, 64-bit, from a sizeof probe against the pgvector 0.8.7 headers.
/// sizeof(HnswElementData) (hnsw.h:182-198).
pub const HNSW_ELEMENT_DATA_SIZE: usize = 128;
/// sizeof(HnswSearchCandidate) (hnsw.h:216-222).
pub const HNSW_SEARCH_CANDIDATE_SIZE: usize = 64;
/// sizeof(pairingheap) (lib/pairingheap.h).
pub const PAIRINGHEAP_SIZE: usize = 24;
/// sizeof(HnswUnvisited) (hnsw.h:404-408).
pub const HNSW_UNVISITED_SIZE: usize = 8;

// ---------------------------------------------------------------------------
// aset.c accounting model.
// ---------------------------------------------------------------------------

// aset.c:83-105, 64-bit, no MEMORY_CONTEXT_CHECKING (the reference build).
const ALLOC_MINBITS: u32 = 3;
const ALLOCSET_NUM_FREELISTS: usize = 11;
const ALLOC_CHUNK_LIMIT: usize = 1 << (ALLOCSET_NUM_FREELISTS - 1 + ALLOC_MINBITS as usize);
const ALLOC_CHUNK_FRACTION: usize = 4;
// MAXALIGN(sizeof(AllocBlockData)): aset, prev, next, freeptr, endptr.
const ALLOC_BLOCKHDRSZ: usize = 40;
// sizeof(MemoryChunk).
const ALLOC_CHUNKHDRSZ: usize = 8;
// MAXALIGN(sizeof(AllocSetContext)): MemoryContextData (80) + blocks (8) +
// freelist[11] (88) + 4 x uint32 (16) + freeListIndex (4) + pad (4).
const ALLOC_SET_CONTEXT_SIZE: usize = 200;

#[inline]
fn maxalign(n: usize) -> usize {
    (n + 7) & !7
}

// aset.c:277 AllocSetFreeIndex.
#[inline]
fn alloc_set_free_index(size: usize) -> usize {
    if size > (1 << ALLOC_MINBITS) {
        (usize::BITS - ((size - 1) >> ALLOC_MINBITS).leading_zeros()) as usize
    } else {
        0
    }
}

#[inline]
fn chunk_size_from_free_list_idx(fidx: usize) -> usize {
    1usize << (fidx as u32 + ALLOC_MINBITS)
}

/// Byte-for-byte model of an aset.c AllocSet's `mem_allocated` (what
/// MemoryContextMemAllocated(ctx, false) returns). Tracks only what that
/// figure depends on: the active block's free tail, the per-class freelist
/// populations, the next block size, and the large-chunk blocks.
#[derive(Clone, Debug)]
pub struct AllocSetModel {
    init_block_size: usize,
    max_block_size: usize,
    alloc_chunk_limit: usize,
    keeper_size: usize,
    mem_allocated: usize,
    next_block_size: usize,
    // set->blocks->endptr - set->blocks->freeptr
    avail: usize,
    nfree: [u32; ALLOCSET_NUM_FREELISTS],
}

impl AllocSetModel {
    /// aset.c:347 AllocSetContextCreateInternal (fresh or recycled: same state).
    pub fn new(min_context_size: usize, init_block_size: usize, max_block_size: usize) -> Self {
        let mut first_block_size = ALLOC_SET_CONTEXT_SIZE + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
        if min_context_size != 0 {
            first_block_size = first_block_size.max(min_context_size);
        } else {
            first_block_size = first_block_size.max(init_block_size);
        }
        let mut alloc_chunk_limit = ALLOC_CHUNK_LIMIT;
        while alloc_chunk_limit + ALLOC_CHUNKHDRSZ
            > (max_block_size - ALLOC_BLOCKHDRSZ) / ALLOC_CHUNK_FRACTION
        {
            alloc_chunk_limit >>= 1;
        }
        let mut m = AllocSetModel {
            init_block_size,
            max_block_size,
            alloc_chunk_limit,
            keeper_size: first_block_size,
            mem_allocated: 0,
            next_block_size: 0,
            avail: 0,
            nfree: [0; ALLOCSET_NUM_FREELISTS],
        };
        m.reset();
        m
    }

    /// aset.c:537 AllocSetReset: keeper block only, freelists cleared.
    pub fn reset(&mut self) {
        self.mem_allocated = self.keeper_size;
        self.avail = self.keeper_size - ALLOC_SET_CONTEXT_SIZE - ALLOC_BLOCKHDRSZ;
        self.nfree = [0; ALLOCSET_NUM_FREELISTS];
        self.next_block_size = self.init_block_size;
    }

    /// MemoryContextMemAllocated(context, false).
    pub fn mem_allocated(&self) -> usize {
        self.mem_allocated
    }

    /// aset.c:967 AllocSetAlloc.
    pub fn palloc(&mut self, size: usize) {
        if size > self.alloc_chunk_limit {
            // aset.c:696 AllocSetAllocLarge.
            self.mem_allocated += maxalign(size) + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
            return;
        }
        let fidx = alloc_set_free_index(size);
        if self.nfree[fidx] > 0 {
            self.nfree[fidx] -= 1;
            return;
        }
        let chunk_size = chunk_size_from_free_list_idx(fidx);
        if self.avail >= chunk_size + ALLOC_CHUNKHDRSZ {
            self.avail -= chunk_size + ALLOC_CHUNKHDRSZ;
            return;
        }
        // aset.c:819 AllocSetAllocFromNewBlock: carve the old block's tail
        // into freelist chunks, then start a new block.
        while self.avail >= (1 << ALLOC_MINBITS) + ALLOC_CHUNKHDRSZ {
            let mut availchunk = self.avail - ALLOC_CHUNKHDRSZ;
            let mut a_fidx = alloc_set_free_index(availchunk);
            if availchunk != chunk_size_from_free_list_idx(a_fidx) {
                a_fidx -= 1;
                availchunk = chunk_size_from_free_list_idx(a_fidx);
            }
            self.nfree[a_fidx] += 1;
            self.avail -= availchunk + ALLOC_CHUNKHDRSZ;
        }
        let mut blksize = self.next_block_size;
        self.next_block_size = (self.next_block_size << 1).min(self.max_block_size);
        let required_size = chunk_size + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
        while blksize < required_size {
            blksize <<= 1;
        }
        self.mem_allocated += blksize;
        self.avail = blksize - ALLOC_BLOCKHDRSZ - chunk_size - ALLOC_CHUNKHDRSZ;
    }

    /// aset.c:1062 AllocSetFree of a chunk palloc'd with `size`.
    pub fn pfree(&mut self, size: usize) {
        if size > self.alloc_chunk_limit {
            self.mem_allocated -= maxalign(size) + ALLOC_BLOCKHDRSZ + ALLOC_CHUNKHDRSZ;
            return;
        }
        self.nfree[alloc_set_free_index(size)] += 1;
    }

    /// aset.c:1188 AllocSetRealloc, small/small and small->large cases used here.
    pub fn repalloc(&mut self, old_size: usize, new_size: usize) {
        if old_size <= self.alloc_chunk_limit
            && chunk_size_from_free_list_idx(alloc_set_free_index(old_size)) >= new_size
        {
            return;
        }
        assert!(old_size <= self.alloc_chunk_limit, "large-chunk realloc not modeled");
        self.palloc(new_size);
        self.pfree(old_size);
    }
}

// ---------------------------------------------------------------------------
// list.c allocation shape of a List built by lappend.
// ---------------------------------------------------------------------------

/// offsetof(List, initial_elements).
const LIST_HEADER_SIZE: u32 = 24;
/// sizeof(ListCell).
const LIST_CELL_SIZE: u32 = 8;
/// list.c:48 LIST_HEADER_OVERHEAD.
const LIST_HEADER_OVERHEAD: u32 = (LIST_HEADER_SIZE - 1) / LIST_CELL_SIZE + 1;

/// The tmpCtx footprint of one C `List *` that is only ever grown by
/// lappend (list.c new_list / enlarge_list) and emptied by list_delete_last.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListShape {
    len: u32,
    max_len: u32,
    // initial_max: cells allocated with the header (new_list); 0 for NIL.
    initial_max: u32,
    inline: bool,
}

impl ListShape {
    pub const NIL: ListShape = ListShape { len: 0, max_len: 0, initial_max: 0, inline: true };

    pub fn len(&self) -> u32 {
        self.len
    }

    /// list.c lappend.
    pub fn lappend(&mut self, mem: &mut AllocSetModel) {
        if self.len == 0 {
            // list.c:117 new_list(T_List, 1).
            let max = (1 + LIST_HEADER_OVERHEAD).max(8).next_power_of_two() - LIST_HEADER_OVERHEAD;
            mem.palloc((LIST_HEADER_SIZE + max * LIST_CELL_SIZE) as usize);
            *self = ListShape { len: 1, max_len: max, initial_max: max, inline: true };
            return;
        }
        if self.len >= self.max_len {
            // list.c:160 enlarge_list(list, length + 1).
            let new_max = (self.len + 1).max(16).next_power_of_two();
            if self.inline {
                mem.palloc((new_max * LIST_CELL_SIZE) as usize);
            } else {
                mem.repalloc((self.max_len * LIST_CELL_SIZE) as usize, (new_max * LIST_CELL_SIZE) as usize);
            }
            self.max_len = new_max;
            self.inline = false;
        }
        self.len += 1;
    }

    /// A List built by `n` lappends onto NIL (e.g. HnswSearchLayer's w).
    pub fn lappend_n(n: usize, mem: &mut AllocSetModel) -> ListShape {
        let mut l = ListShape::NIL;
        for _ in 0..n {
            l.lappend(mem);
        }
        l
    }

    /// list.c list_delete_last: frees the List once it would become empty.
    pub fn delete_last(&mut self, mem: &mut AllocSetModel) {
        assert!(self.len > 0, "list_delete_last on NIL");
        if self.len <= 1 {
            // list.c list_free_private.
            if !self.inline {
                mem.pfree((self.max_len * LIST_CELL_SIZE) as usize);
            }
            mem.pfree((LIST_HEADER_SIZE + self.initial_max * LIST_CELL_SIZE) as usize);
            *self = ListShape::NIL;
        } else {
            self.len -= 1;
        }
    }
}

// ---------------------------------------------------------------------------
// tidhash: simplehash.h instantiated as in hnswutils.c:41-66.
// ---------------------------------------------------------------------------

const SH_FILLFACTOR: f64 = 0.9;
const SH_MAX_FILLFACTOR: f64 = 0.98;
const SH_GROW_MAX_DIB: u32 = 25;
const SH_GROW_MAX_MOVE: u32 = 150;
const SH_GROW_MIN_FILLFACTOR: f64 = 0.1;
const SH_MAX_SIZE: u64 = (u32::MAX as u64) + 1;
const TID_EMPTY: u64 = u64::MAX;
/// sizeof(TidHashEntry): ItemPointerData (6) + status (1), padded to 8.
const TID_HASH_ENTRY_SIZE: usize = 8;
/// sizeof(tidhash_hash).
const TID_HASH_HEADER_SIZE: usize = 48;

/// hash_tid's key (hnswutils.c:43-56): ItemPointerData (bi_hi, bi_lo,
/// ip_posid) little-endian in the low 6 bytes of a zeroed uint64.
#[inline]
fn tid_key(blkno: u32, offno: u16) -> u64 {
    ((blkno >> 16) as u64) | (((blkno & 0xffff) as u64) << 16) | ((offno as u64) << 32)
}

#[inline]
fn hash_tid(key: u64) -> u32 {
    hashfn::murmurhash64(key) as u32
}

/// C's visited_hash.tids (simplehash: robin hood, no stored hash). Used as
/// the scan's real visited set, so its growth points are C's.
pub struct TidHash {
    data: Vec<u64>,
    sizemask: u32,
    members: u32,
    grow_threshold: u32,
}

impl TidHash {
    fn compute_size(newsize: u64) -> u64 {
        newsize.max(2).next_power_of_two()
    }

    /// SH_CREATE(ctx, nelements): charges the header and bucket array.
    pub fn create(nelements: u32, mem: Option<&mut AllocSetModel>) -> Self {
        let size = Self::compute_size((SH_MAX_SIZE as f64).min(nelements as f64 / SH_FILLFACTOR) as u64);
        if let Some(mem) = mem {
            mem.palloc(TID_HASH_HEADER_SIZE);
            mem.palloc(TID_HASH_ENTRY_SIZE * size as usize);
        }
        let mut t = TidHash { data: vec![TID_EMPTY; size as usize], sizemask: 0, members: 0, grow_threshold: 0 };
        t.update_parameters(size);
        t
    }

    pub fn size(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn members(&self) -> u32 {
        self.members
    }

    fn update_parameters(&mut self, size: u64) {
        self.sizemask = (size - 1) as u32;
        self.grow_threshold = if size == SH_MAX_SIZE {
            (size as f64 * SH_MAX_FILLFACTOR) as u32
        } else {
            (size as f64 * SH_FILLFACTOR) as u32
        };
    }

    #[inline]
    fn initial_bucket(&self, hash: u32) -> u32 {
        hash & self.sizemask
    }

    #[inline]
    fn distance_from_optimal(&self, optimal: u32, bucket: u32) -> u32 {
        if optimal <= bucket {
            bucket - optimal
        } else {
            (self.size() as u32).wrapping_add(bucket).wrapping_sub(optimal)
        }
    }

    /// SH_GROW: allocate the new array, rehash, free the old one.
    fn grow(&mut self, newsize: u64, mut mem: Option<&mut AllocSetModel>) {
        let oldsize = self.size();
        let newsize = Self::compute_size(newsize);
        if let Some(mem) = mem.as_deref_mut() {
            mem.palloc(TID_HASH_ENTRY_SIZE * newsize as usize);
        }
        let old = core::mem::replace(&mut self.data, vec![TID_EMPTY; newsize as usize]);
        self.update_parameters(newsize);
        let mut startelem = 0u32;
        for i in 0..oldsize as u32 {
            let k = old[i as usize];
            if k == TID_EMPTY {
                startelem = i;
                break;
            }
            if self.initial_bucket(hash_tid(k)) == i {
                startelem = i;
                break;
            }
        }
        let mut copyelem = startelem;
        for _ in 0..oldsize {
            let k = old[copyelem as usize];
            if k != TID_EMPTY {
                let mut cur = self.initial_bucket(hash_tid(k));
                while self.data[cur as usize] != TID_EMPTY {
                    cur = (cur + 1) & self.sizemask;
                }
                self.data[cur as usize] = k;
            }
            copyelem += 1;
            if copyelem as u64 >= oldsize {
                copyelem = 0;
            }
        }
        if let Some(mem) = mem {
            mem.pfree(TID_HASH_ENTRY_SIZE * oldsize as usize);
        }
    }

    /// tidhash_insert: returns `found`.
    pub fn insert(&mut self, blkno: u32, offno: u16, mut mem: Option<&mut AllocSetModel>) -> bool {
        let key = tid_key(blkno, offno);
        let hash = hash_tid(key);
        'restart: loop {
            let mut insertdist = 0u32;
            if self.members >= self.grow_threshold {
                assert!(self.size() != SH_MAX_SIZE, "hash table size exceeded");
                self.grow(self.size() * 2, mem.as_deref_mut());
            }
            let startelem = self.initial_bucket(hash);
            let mut curelem = startelem;
            loop {
                let occ = self.data[curelem as usize];
                if occ == TID_EMPTY {
                    self.members += 1;
                    self.data[curelem as usize] = key;
                    return false;
                }
                if occ == key {
                    return true;
                }
                let curoptimal = self.initial_bucket(hash_tid(occ));
                let curdist = self.distance_from_optimal(curoptimal, curelem);
                if insertdist > curdist {
                    let mut emptyelem = curelem;
                    let mut emptydist = 0u32;
                    loop {
                        emptyelem = (emptyelem + 1) & self.sizemask;
                        if self.data[emptyelem as usize] == TID_EMPTY {
                            break;
                        }
                        emptydist += 1;
                        if emptydist > SH_GROW_MAX_MOVE
                            && (self.members as f64 / self.size() as f64) >= SH_GROW_MIN_FILLFACTOR
                        {
                            self.grow_threshold = 0;
                            continue 'restart;
                        }
                    }
                    let mut moveelem = emptyelem;
                    while moveelem != curelem {
                        let prev = moveelem.wrapping_sub(1) & self.sizemask;
                        self.data[moveelem as usize] = self.data[prev as usize];
                        moveelem = prev;
                    }
                    self.members += 1;
                    self.data[curelem as usize] = key;
                    return false;
                }
                curelem = (curelem + 1) & self.sizemask;
                insertdist += 1;
                if insertdist > SH_GROW_MAX_DIB
                    && (self.members as f64 / self.size() as f64) >= SH_GROW_MIN_FILLFACTOR
                {
                    self.grow_threshold = 0;
                    continue 'restart;
                }
            }
        }
    }
}
```

- [ ] **Step 5: Lock file and tests**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
```
Expected: `hashfn` is added to `types_hnsw`.

Run: `UT tmpctx`
Expected: 5 passed (`list_of_40_matches_c_trace_shape`, `keeper_block_holds_58_elements_then_8k_block`, `fresh_scan_context_is_one_keeper_block`, `tidhash_set_semantics`, `replay_c_trace`).

- [ ] **Step 6: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
git add crates/_support/types/types_hnsw Cargo.lock
git commit -q -F - <<'EOF'
types_hnsw: byte-exact model of hnswscan's tmpCtx (AllocSet, List, tidhash)

AllocSetModel reproduces MemoryContextMemAllocated for the scan context;
ListShape and TidHash reproduce the allocation shapes of C's Lists and of
simplehash's tidhash. Replays a recorded C trace (523 checkpoints).

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 9: Exact iterative-scan memory cap, and the stop-point tier

**Files:**
- Create: `scripts/pgvector/run-iterscan.sh`
- Modify: `scripts/pgvector/run-all.sh` and `scripts/pgvector/tests/harness_test.sh`
- Modify: `crates/_support/types/types_hnsw/src/lib.rs` (`HnswScanOpaqueData`) and `crates/_support/types/types_hnsw/Cargo.toml` (drop `rustc-hash`)
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs` (`search_layer_disk`, the `find_element_neighbors` call sites, delete `Visited`)
- Modify: `crates/contrib/pgvector_hnsw/src/scan.rs` (`hnswbeginscan`, `hnswrescan`, `hnswendscan`, `get_scan_items`, `resume_scan_items`, `hnswgettuple`; delete `SCAN_TUPLE_MEM`)
- Modify: `crates/contrib/pgvector_hnsw/src/lib.rs` (header `DIVERGENCE` text)
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: Task 8's model API, Task 7's heaps and `ScanDiscardedHeap`, and Task 5's seed.
- Produces:
  - `HnswScanOpaqueData { .., tmp_ctx: AllocSetModel, w_list: ListShape, visited: Option<TidHash>, discarded: Option<ScanDiscardedHeap> }`, with `mem_used` removed.
  - `search_layer_disk(pool, q, ep, ef, lc, index, support, m, inserting, skip_element, visited: Option<&mut Option<TidHash>>, discarded: Option<&mut Option<ScanDiscardedHeap>>, init_visited: bool, tuples: Option<&mut i64>, tmp_ctx: Option<&mut AllocSetModel>)`. `visited: None` is C's `v == NULL`, a local set. Discarded is allocated at `InitVisited`, as in C.
  - `run-iterscan.sh {pgrust|ref} | --list`, which writes `$PGV_WORK/iterscan/<mode>/summary.tsv`.

- [ ] **Step 1: Write the tier and its self-tests**

Create `scripts/pgvector/run-iterscan.sh`:
```bash
#!/usr/bin/env bash
# Iterative-scan stop-point tier (spec §8.2). Builds the same seeded, tie-free
# HNSW index on the seeded C reference (the oracle) and on the subject, then
# runs iterative index scans that stop on hnsw.max_scan_tuples or on the scan
# memory cap, MemoryContextMemAllocated(so->tmpCtx) > work_mem *
# hnsw.scan_mem_multiplier (hnswscan.c:264). The filter matches 10 of 1000
# rows and each scan asks for 11, so every scan runs until a limit stops it;
# the rows it returns and "Rows Removed by Filter" show where it stopped.
#
# usage: run-iterscan.sh {pgrust|ref}
#        run-iterscan.sh --list
#   pgrust: the subject is pgrust (SET pgrust.hnsw_build_seed = 42).
#   ref:    the subject is a second, fresh seeded C cluster (must always pass).
set -euo pipefail
. "$(dirname "$0")/common.sh"

# name:hnsw.iterative_scan:work_mem:hnsw.scan_mem_multiplier:hnsw.max_scan_tuples:query
CASES="relaxed_64k:relaxed_order:64kB:1:20000:knn
relaxed_64k_x2:relaxed_order:64kB:2:20000:knn
strict_64k:strict_order:64kB:1:20000:knn
relaxed_128k:relaxed_order:128kB:1:20000:knn
relaxed_tuples:relaxed_order:4MB:1:300:knn
strict_tuples:strict_order:4MB:1:300:knn
lateral_64k:relaxed_order:64kB:1:20000:lateral"

# Tie-free Golomb-ruler data (see run-bytecmp.sh l2_golomb): the graph and
# every scan order follow from the seed-42 level stream alone.
setup_sql() {
  cat <<'SQL'
SET max_parallel_maintenance_workers = 0;
SET maintenance_work_mem = '64MB';
SET pgrust.hnsw_build_seed = 42;
CREATE EXTENSION vector;
CREATE TABLE tst (i int4, v vector(3));
INSERT INTO tst SELECT i, ARRAY[2*1009*k + (k*k) % 1009, 0, 0]::vector FROM (SELECT i, (i * 37) % 1009 AS k FROM generate_series(0, 999) i) s;
CREATE INDEX tst_v_idx ON tst USING hnsw (v vector_l2_ops);
SQL
}

query_sql() {
  case "$1" in
    knn) echo "SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> '[1000000,0,0]' LIMIT 11" ;;
    # Three rescans of one index scan (review focus 4: each must start from a
    # reset tmpCtx and fresh visited/discarded state).
    lateral) echo "SELECT q.x, t.i FROM (VALUES (1000000), (250000), (1750000)) q(x) CROSS JOIN LATERAL (SELECT i FROM tst WHERE i % 100 = 0 ORDER BY v <-> ARRAY[q.x, 0, 0]::vector LIMIT 11) t" ;;
    *) die "unknown query '$1'" ;;
  esac
}

case_sql() { # case_sql ITERATIVE WORK_MEM MULTIPLIER MAX_TUPLES QUERY
  cat <<SQL
SET enable_seqscan = off;
SET hnsw.iterative_scan = $1;
SET work_mem = '$2';
SET hnsw.scan_mem_multiplier = $3;
SET hnsw.max_scan_tuples = $4;
EXPLAIN (ANALYZE, COSTS OFF, TIMING OFF, SUMMARY OFF, BUFFERS OFF) $(query_sql "$5");
$(query_sql "$5");
SQL
}

# run_side LABEL KIND: fresh cluster, build the index, run every case, stop.
# Keeps the "Rows Removed by Filter" lines and the result rows.
run_side() {
  local label="$1" kind="$2" port name iter wm mult maxt query
  port="$(port_for "$kind")"
  server_stop "$kind"
  rm -rf "$out/data/$label"
  server_start "$kind" "$out/data/$label" "$port"
  setup_sql | pgv_psql "$port" -f - >"$out/$label.setup.log" 2>&1 ||
    die "$label ($kind): setup failed; see $out/$label.setup.log"
  if [ "$kind" = seeded ] && ! grep -q 'INFO:  memory: ' "$out/$label.setup.log"; then
    die "$label: index was not built by the seeded C pgvector (no 'INFO:  memory:' line)"
  fi
  while IFS=: read -r name iter wm mult maxt query; do
    case_sql "$iter" "$wm" "$mult" "$maxt" "$query" >"$out/$label.$name.sql"
    pgv_psql "$port" -At -f "$out/$label.$name.sql" 2>"$out/$label.$name.err" |
      sed -n -e 's/^ *\(Rows Removed by Filter: [0-9]*\)$/\1/p' -e '/^[0-9][0-9|]*$/p' \
        >"$out/$label.$name.out" ||
      die "$label ($kind): case $name failed; see $out/$label.$name.err"
  done <<EOF
$CASES
EOF
  server_stop "$kind"
}

if [ "${1:-}" = --list ]; then
  echo "$CASES" | cut -d: -f1
  exit 0
fi
[ "$#" -eq 1 ] || die "usage: run-iterscan.sh {pgrust|ref}"
mode="$1"
case "$mode" in
  pgrust) subject=pgrust ;;
  ref) subject=seeded ;;
  *) die "unknown mode '$mode' (expected pgrust or ref)" ;;
esac
out="$PGV_WORK/iterscan/$mode"
rm -rf "$out"
mkdir -p "$out/diff"
trap 'server_stop seeded; server_stop "$subject"' EXIT

run_side oracle seeded
run_side subject "$subject"

: >"$out/summary.tsv"
total=0
passed=0
while IFS=: read -r name _; do
  total=$((total + 1))
  if [ -s "$out/oracle.$name.out" ] && cmp -s "$out/oracle.$name.out" "$out/subject.$name.out"; then
    result=ok
    passed=$((passed + 1))
  else
    result=FAIL
    diff "$out/oracle.$name.out" "$out/subject.$name.out" >"$out/diff/$name.txt" || true
  fi
  printf '%s\t%s\n' "$name" "$result" | tee -a "$out/summary.tsv"
done <<EOF
$CASES
EOF
echo "iterscan ($mode, subject $subject): $passed/$total identical; diffs in $out/diff"
[ "$passed" -eq "$total" ]
```
Run `chmod +x scripts/pgvector/run-iterscan.sh`.

In `scripts/pgvector/tests/harness_test.sh`, insert above `# --- runner ---`:
```bash
# Iterative-scan tier, C vs C: two fresh seeded clusters stop identically.
test_iterscan_ref() {
  local want n
  want="$("$here/../run-iterscan.sh" --list | wc -l | tr -d ' ')"
  if "$here/../run-iterscan.sh" ref >/dev/null 2>&1; then
    pass "iterscan ref: exit 0"
  else
    fail "iterscan ref: exit 0 (see $PGV_WORK/iterscan/ref)"
  fi
  n="$(grep -c $'\tok$' "$PGV_WORK/iterscan/ref/summary.tsv" 2>/dev/null || true)"
  if [ "$n" = "$want" ]; then pass "iterscan ref: $want ok"; else fail "iterscan ref: '$n' ok, want $want"; fi
}

# The cases must stop where they claim (reads the C-vs-C run above): the 64kB
# case on the memory cap, the 300-tuple case on hnsw.max_scan_tuples, and a
# doubled multiplier must scan further. 990 rows fail the filter in total.
test_iterscan_cases() {
  local a b t
  removed() { sed -n 's/^Rows Removed by Filter: //p' "$PGV_WORK/iterscan/ref/oracle.$1.out" | head -1; }
  a="$(removed relaxed_64k)"
  b="$(removed relaxed_64k_x2)"
  t="$(removed relaxed_tuples)"
  if [ -n "$a" ] && [ "$a" -lt 990 ]; then pass "iterscan relaxed_64k stops early ($a removed)"; else fail "iterscan relaxed_64k: removed '$a', want < 990"; fi
  if [ -n "$b" ] && [ -n "$a" ] && [ "$b" -gt "$a" ]; then pass "iterscan: multiplier x2 scans further ($a -> $b)"; else fail "iterscan: multiplier x2 removed '$b', want > '$a'"; fi
  if [ -n "$t" ] && [ "$t" -lt 990 ]; then pass "iterscan relaxed_tuples stops early ($t removed)"; else fail "iterscan relaxed_tuples: removed '$t', want < 990"; fi
}
```
In `run-all.sh`:
- after the `run-bytecmp.sh` line, add `"$here/run-iterscan.sh" "$mode" || true`;
- change the loop to `for tier in regress tap bytecmp iterscan; do`.

- [ ] **Step 2: See C vs C pass and pgrust fail**

```bash
scripts/pgvector/tests/harness_test.sh test_iterscan_ref test_iterscan_cases
scripts/pgvector/run-iterscan.sh pgrust || true
```
Expected:
- The self-tests print `all passed`.
- The pgrust run reports `relaxed_tuples` and `strict_tuples` as `ok`. They stop on the tuple limit and need no memory model, which also proves the EXPLAIN lines compare cleanly between the servers.
- The 64kB and 128kB cases are `FAIL` because the 200-bytes-per-tuple estimate trips at a different point.
- If the tuple cases FAIL too, stop: the graphs or the EXPLAIN output differ. Check `run-bytecmp.sh pgrust l2_golomb` first.

- [ ] **Step 3: `types_hnsw`: the scan state holds the model**

In `crates/_support/types/types_hnsw/src/lib.rs`, replace the `mem_used` field (with its comment) and the `visited` field:
```rust
    // so->tmpCtx's MemoryContextMemAllocated, modeled (tmpctx.rs): the
    // iterative scan stops when it exceeds max_memory (hnswscan.c:264).
    pub tmp_ctx: AllocSetModel,
```
```rust
    // The allocation shape of the C List so->w in tmpCtx.
    pub w_list: ListShape,
    // so->v.tids (hnswscan.c:60): NULL until GetScanItems' layer-0 search.
    pub visited: Option<TidHash>,
```
Remove `rustc-hash = ...` from `crates/_support/types/types_hnsw/Cargo.toml` (now unused), then run `cargo update --workspace --offline`.

- [ ] **Step 4: `utils.rs`: `search_layer_disk` with C's visited set and allocation points**

Replace `search_layer_disk` (with its comment) by:
```rust
// HnswSearchLayer (on-disk form, index != NULL; hnswutils.c:828-990). Returns
// w emptied from the furthest-first heap: furthest first, nearest last (C list
// order). `visited: None` is C's v == NULL (a local set, always initialized);
// `discarded` is C's pairingheap **discarded, allocated at InitVisited.
// `tmp_ctx` (scans only) is charged at C's tmpCtx allocation points; the
// caller charges the returned w list (C's final lappends).
#[allow(clippy::too_many_arguments)]
pub fn search_layer_disk<'t>(
    pool: &mut ElementPool<'t>,
    q: Option<Datum>,
    ep: PgVec<'t, SearchCandidate>,
    ef: i32,
    lc: i32,
    index: &Relation<'_>,
    support: &mut HnswSupport,
    m: i32,
    inserting: bool,
    skip_element: Option<(BlockNumber, u16)>,
    visited: Option<&mut Option<TidHash>>,
    discarded: Option<&mut Option<ScanDiscardedHeap>>,
    init_visited: bool,
    mut tuples: Option<&mut i64>,
    mut tmp_ctx: Option<&mut AllocSetModel>,
) -> PgResult<PgVec<'t, SearchCandidate>> {
    let lm = hnsw_get_layer_m(m, lc);
    // pairingheap_allocate C and W; palloc_array_checked(HnswUnvisited, lm)
    // (hnswutils.c:831-832, 839).
    if let Some(mem) = tmp_ctx.as_deref_mut() {
        mem.palloc(PAIRINGHEAP_SIZE);
        mem.palloc(PAIRINGHEAP_SIZE);
        mem.palloc(HNSW_UNVISITED_SIZE * lm as usize);
    }
    let mut c_heap = nearest_candidate_heap();
    let mut w_heap = furthest_candidate_heap();
    let mut wlen: i32 = 0;

    // v == NULL: a local set, always initialized (hnswutils.c:843-847).
    let mut local_visited: Option<TidHash> = None;
    let (visited, init_visited) = match visited {
        Some(v) => (v, init_visited),
        None => (&mut local_visited, true),
    };
    let mut discarded = discarded;
    if init_visited {
        // InitVisited (hnswutils.c:849-855, 677-680): tidhash_create(ef * m * 2).
        *visited = Some(TidHash::create((ef * m * 2) as u32, tmp_ctx.as_deref_mut()));
        if let Some(slot) = discarded.as_deref_mut() {
            if let Some(mem) = tmp_ctx.as_deref_mut() {
                mem.palloc(PAIRINGHEAP_SIZE);
            }
            *slot = Some(new_scan_discarded_heap());
        }
    }
    let visited = visited.as_mut().expect("visited set initialized");
    let mut discarded: Option<&mut ScanDiscardedHeap> =
        discarded.map(|slot| slot.as_mut().expect("discarded heap allocated at InitVisited"));

    // CountElement: skip elements being deleted when vacuuming.
    let count_element = |pool: &ElementPool<'_>, e: u32| -> bool {
        if skip_element.is_none() {
            return true;
        }
        pool.get(e).heaptids_len != 0
    };

    for sc in ep.iter() {
        if init_visited {
            // AddToVisited (hnswutils.c:866-871).
            let e = pool.get(sc.element);
            visited.insert(e.blkno, e.offno, tmp_ctx.as_deref_mut());
            if let Some(t) = tuples.as_deref_mut() {
                *t += 1;
            }
        }
        c_heap.add((sc.distance, sc.element));
        w_heap.add((sc.distance, sc.element));
        if count_element(pool, sc.element) {
            wlen += 1;
        }
    }
    drop(ep);

    let mut unvisited: Vec<(BlockNumber, u16)> = Vec::with_capacity(lm as usize);
    let mut tidbuf = [[0u8; 6]; 200];

    while let Some((c_dist, c_elem)) = c_heap.remove_first() {
        let (f_dist, _) = *w_heap.first().expect("W nonempty while C nonempty");
        if c_dist > f_dist {
            break;
        }

        // HnswLoadUnvisitedFromDisk (hnswutils.c:800-822).
        unvisited.clear();
        if load_neighbor_tids(pool, c_elem, &mut tidbuf[..lm as usize], index, m, lm, lc)? {
            for tid in tidbuf[..lm as usize].iter() {
                if !itemptr_is_valid(tid) {
                    break;
                }
                let (blkno, offno) = itemptr_decode(tid);
                if !visited.insert(blkno, offno, tmp_ctx.as_deref_mut()) {
                    unvisited.push((blkno, offno));
                }
            }
        }

        if let Some(t) = tuples.as_deref_mut() {
            *t += unvisited.len() as i64;
        }

        for &(blkno, offno) in unvisited.iter() {
            let always_add = wlen < ef;
            let (f_dist, _) = *w_heap.first().expect("W nonempty");

            let id = pool.from_block(blkno, offno);
            let mut e_distance = 0.0f64;
            let max_distance = if always_add || discarded.is_some() {
                None
            } else {
                Some(f_dist)
            };
            let loaded = load_element(
                pool,
                id,
                Some(&mut e_distance),
                q,
                index,
                support,
                inserting,
                max_distance,
            )?;
            if !loaded {
                continue;
            }
            // HnswInitElementFromBlock for a loaded element (hnswutils.c:567-570).
            if let Some(mem) = tmp_ctx.as_deref_mut() {
                mem.palloc(HNSW_ELEMENT_DATA_SIZE);
            }

            if !(e_distance < f_dist || always_add) {
                if let Some(dh) = discarded.as_deref_mut() {
                    // HnswInitSearchCandidate (hnswutils.c:944).
                    if let Some(mem) = tmp_ctx.as_deref_mut() {
                        mem.palloc(HNSW_SEARCH_CANDIDATE_SIZE);
                    }
                    dh.add(pool.scan_element(id, e_distance));
                }
                continue;
            }

            // Make robust to issues.
            if (pool.get(id).level as i32) < lc {
                continue;
            }

            // HnswInitSearchCandidate (hnswutils.c:956).
            if let Some(mem) = tmp_ctx.as_deref_mut() {
                mem.palloc(HNSW_SEARCH_CANDIDATE_SIZE);
            }
            c_heap.add((e_distance, id));
            w_heap.add((e_distance, id));

            if count_element(pool, id) {
                wlen += 1;
                if wlen > ef {
                    let (d_dist, d_elem) = w_heap.remove_first().expect("W nonempty");
                    if let Some(dh) = discarded.as_deref_mut() {
                        dh.add(pool.scan_element(d_elem, d_dist));
                    }
                }
            }
        }
    }

    let mut w: PgVec<'t, SearchCandidate> = mcx::vec_with_capacity_in_infallible(pool.mcx, 0);
    while let Some((d, x)) = w_heap.remove_first() {
        w.push(SearchCandidate { element: x, distance: d });
    }
    Ok(w)
}
```
Then:
- check that `HnswLoadUnvisitedFromDisk`'s visited insert order matches C (one insert per valid TID, in TID order);
- delete `pub type Visited<'v> = ...`;
- in `find_element_neighbors`, delete both `let mut visited: Visited<'_> = PgFxHashMap::with_capacity_and_hasher_in(...);` statements, and change both calls to pass `None, None, true, None, None` for `visited, discarded, init_visited, tuples, tmp_ctx`. That is C's `HnswSearchLayer(..., NULL, NULL, true, NULL)`.

Remove imports that are now unused (`PgFxHashMap`) only if the compiler flags them.

- [ ] **Step 5: `scan.rs`: charge where C allocates**

- **Constants and imports.** Delete `const SCAN_TUPLE_MEM` and its comment. Drop `PgFxHashMap` from the `mcx` import if it's now unused.
- **`hnswbeginscan`.** In the `HnswScanOpaqueData` literal, replace `mem_used: 0,` with `tmp_ctx: AllocSetModel::new(0, 8 * 1024, 256 * 1024),` and add `// hnswscan.c:154-156` above it. Replace `visited: Default::default(),` with `w_list: ListShape::NIL,` and `visited: None,`.
- **`hnswrescan`.** Replace `so.mem_used = 0;` and the reset block with:
  ```rust
      so.previous_distance = f64::NEG_INFINITY;
      // hnswrescan (hnswscan.c:176-182): v and discarded live in tmpCtx, which
      // is reset. The owned values are dropped so memory stays bounded across
      // arbitrarily many rescans.
      so.tmp_ctx.reset();
      so.value = None;
      so.w = Vec::new();
      so.w_list = ListShape::NIL;
      so.visited = None;
      so.discarded = None;
  ```
- **`hnswendscan`.** Set `so.w_list = ListShape::NIL;` and `so.visited = None;` in place of `so.visited = Default::default();`.
- **`get_scan_items`.** From `let ep_id = pool.from_block(...)` to the end of the function, replace the body with:
  ```rust
      let ep_id = pool.from_block(entry.blkno, entry.offno);
      // HnswGetMetaPageInfo's HnswInitElementFromBlock (hnswutils.c:325).
      so.tmp_ctx.palloc(HNSW_ELEMENT_DATA_SIZE);
      pool.get_mut(ep_id).level = entry.level;
      let mut ep_dist = 0.0f64;
      load_element(&mut pool, ep_id, Some(&mut ep_dist), q, &index, &mut support, false, None)?;
      // HnswEntryCandidate's HnswInitSearchCandidate, then list_make1 (hnswscan.c:52).
      so.tmp_ctx.palloc(HNSW_SEARCH_CANDIDATE_SIZE);
      let _ = ListShape::lappend_n(1, &mut so.tmp_ctx);

      let mut ep: PgVec<'_, SearchCandidate> = mcx::vec_with_capacity_in_infallible(tmcx, 1);
      ep.push(SearchCandidate { element: ep_id, distance: ep_dist });

      let mut lc = entry.level as i32;
      while lc >= 1 {
          // hnswscan.c:54-58: v == NULL; the previous ep List is not freed.
          ep = search_layer_disk(
              &mut pool, q, ep, 1, lc, &index, &mut support, so.m, false, None, None, None,
              true, None, Some(&mut so.tmp_ctx),
          )?;
          let _ = ListShape::lappend_n(ep.len(), &mut so.tmp_ctx);
          lc -= 1;
      }

      let ef_search = guc_tables::vars::hnsw_ef_search.read();
      // C reads the hnsw_iterative_scan GUC at each use (no cached copy).
      let iterative = guc_tables::vars::hnsw_iterative_scan.read() != HNSW_ITERATIVE_SCAN_OFF;
      let mut tuples = so.tuples;
      // hnswscan.c:63: &so->v, and &so->discarded only with iterative scans.
      let w = search_layer_disk(
          &mut pool,
          q,
          ep,
          ef_search,
          0,
          &index,
          &mut support,
          so.m,
          false,
          None,
          Some(&mut so.visited),
          if iterative { Some(&mut so.discarded) } else { None },
          true,
          Some(&mut tuples),
          Some(&mut so.tmp_ctx),
      )?;
      so.tuples = tuples;
      so.w_list = ListShape::lappend_n(w.len(), &mut so.tmp_ctx);
      so.support = support;
      so.w = w.iter().map(|sc| pool.scan_element(sc.element, sc.distance)).collect();
      Ok(())
  ```
  This also deletes Task 7's `so.discarded = iterative.then(...)` line: the heap is now allocated inside the search at `InitVisited`.
- **`resume_scan_items`.** From the `let mut ep: PgVec...` declaration to the end, replace the body with:
  ```rust
      let mut ep: PgVec<'_, SearchCandidate> =
          mcx::vec_with_capacity_in_infallible(tmcx, batch_size as usize);
      let mut ep_list = ListShape::NIL;
      {
          let dh = so.discarded.as_mut().expect("checked");
          for _ in 0..batch_size {
              let Some(e) = dh.remove_first() else { break };
              // ep = lappend(ep, sc) (hnswscan.c:88): sc is an existing
              // candidate, so only the List grows.
              ep_list.lappend(&mut so.tmp_ctx);
              let id = pool.from_block(e.blkno, e.offno);
              let pe = pool.get_mut(id);
              pe.level = e.level;
              pe.version = e.version;
              pe.heaptids = e.heaptids;
              pe.heaptids_len = e.heaptids_len;
              pe.neighbor_page = e.neighbor_page;
              pe.neighbor_offno = e.neighbor_offno;
              ep.push(SearchCandidate { element: id, distance: e.distance });
          }
      }

      let mut tuples = so.tuples;
      // hnswscan.c:91: &so->v and &so->discarded, initVisited = false.
      let w = search_layer_disk(
          &mut pool,
          q,
          ep,
          batch_size,
          0,
          &index,
          &mut support,
          so.m,
          false,
          None,
          Some(&mut so.visited),
          Some(&mut so.discarded),
          false,
          Some(&mut tuples),
          Some(&mut so.tmp_ctx),
      )?;
      so.tuples = tuples;
      so.w_list = ListShape::lappend_n(w.len(), &mut so.tmp_ctx);
      so.support = support;
      so.w = w.iter().map(|sc| pool.scan_element(sc.element, sc.distance)).collect();
      Ok(())
  ```
  This removes the visited copy-in and copy-out. One `TidHash` lives across resumes, as C's `so->v` does. That also removes the cost of copying the whole visited set on every resume.
- **`hnswgettuple`, first call.** Right after `so.value = get_scan_value(&mut support, &orderby)?;` and `so.support = support;`, add:
  ```rust
              // HnswNormValue's palloc0(VECTOR_SIZE(dim)) in tmpCtx (hnswscan.c:115).
              if so.support.normprocinfo.is_some() {
                  if let Some(v) = so.value.as_ref() {
                      so.tmp_ctx.palloc(v.len());
                  }
              }
  ```
- **`hnswgettuple`, loop.**
  - Replace `|| so.mem_used > so.max_memory` with `|| so.tmp_ctx.mem_allocated() > so.max_memory` and add `// hnswscan.c:264` above that `else if`.
  - In the `drain_one` branch, change it to:
    ```rust
                let e = so.discarded.as_mut().expect("some").remove_first().expect("nonempty");
                // so->w = lappend(so->w, ...) (hnswscan.c:270).
                so.w_list.lappend(&mut so.tmp_ctx);
                so.w.push(e);
    ```
  - Replace the empty-heaptids branch with:
    ```rust
            if so.w[last].heaptids_len == 0 {
                so.w.pop();
                // list_delete_last, then pfree(element) and pfree(sc) with
                // iterative scans (hnswscan.c:302-311).
                so.w_list.delete_last(&mut so.tmp_ctx);
                if guc_tables::vars::hnsw_iterative_scan.read() != HNSW_ITERATIVE_SCAN_OFF {
                    so.tmp_ctx.pfree(HNSW_ELEMENT_DATA_SIZE);
                    so.tmp_ctx.pfree(HNSW_SEARCH_CANDIDATE_SIZE);
                }
                continue;
            }
    ```

In `crates/contrib/pgvector_hnsw/src/lib.rs`, replace the header phrase `iterative-scan memory cap approximates C's MemoryContextMemAllocated with per-tuple estimates;` with `iterative-scan memory cap is a byte-exact model of C's so->tmpCtx (types_hnsw::tmpctx), charged at C's allocation points;`.

- [ ] **Step 6: Build and run the unit tests**

```bash
cargo update --workspace --offline
git diff --stat Cargo.lock
```
Expected: `rustc-hash` is removed from `types_hnsw`.

Run: `UT_M1`.
Expected: all pass, with no warnings about unused imports.

- [ ] **Step 7: The stop-point tier, then TAP 043/044 and the regression checks**

```bash
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-iterscan.sh pgrust
scripts/pgvector/run-tap.sh pgrust 043_hnsw_iterative_scan.pl 044_hnsw_iterative_scan_recall.pl 016_hnsw_inserts.pl 017_hnsw_filtering.pl 046_hnsw_vacuum_scan.pl
scripts/pgvector/run-regress.sh pgrust vector_type hnsw_vector
scripts/pgvector/run-bytecmp.sh pgrust
```
Expected:
- `iterscan (pgrust, subject pgrust): 7/7 identical`;
- all five TAP tests `ok` (043 and 044 were FAIL in the M0 baseline);
- both regression tests `ok`;
- bytecmp `10/10`.

If an iterscan case still differs:
- Compare `$PGV_WORK/iterscan/pgrust/oracle.<case>.out` with `subject.<case>.out`.
- Check the charge order against C's event order: `A 128, A 64, A 64 | A 24, A 24, A 128/256, A 48, A 512/16384, (A 24) | … | A 64, A 128, A 256, F 128, A 512, F 256`. The C allocation points are listed in `~/.cache/pgrust/m1-research/iterscan/tmpctx-shadow-instrumentation.patch`.
- Fix the code, not the expected output.

- [ ] **Step 8: Lints and commit**

```bash
bash crates/_support/seams_init/tests/lint-determinism.sh
git add crates/_support/types/types_hnsw crates/contrib/pgvector_hnsw/src/{lib,utils,scan}.rs scripts/pgvector/run-iterscan.sh scripts/pgvector/run-all.sh scripts/pgvector/tests/harness_test.sh Cargo.lock
git commit -q -F - <<'EOF'
pgvector_hnsw: exact iterative-scan memory cap; stop-point tier

The cap charged ~200 bytes per element ever seen, never credited frees
and re-charged resumed candidates, so iterative scans stopped about 8x
early (TAP 043/044). The scan now charges the modeled tmpCtx at C's
allocation points and keeps one tidhash visited set across resumes.
run-iterscan.sh compares stop points with the seeded C build: 7/7.

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

### Task 10: Docs, the full gate, and the M1 report

**Files:**
- Modify: the headers of `crates/contrib/pgvector/src/lib.rs:1-4`, `crates/contrib/pgvector_hnsw/src/lib.rs:1-6` and `crates/contrib/pgvector_hnsw_build/src/lib.rs:1-3`
- Modify: `crates/contrib/pgvector_hnsw/src/utils.rs:65-67` (comment)
- Modify: `CLAUDE.md` (the "Vector search (fork work)" section)
- Create: `docs/superpowers/reports/pgvector-m1-report.md`

**Interfaces:**
- Produces the M1 report. The M2 plan starts from it.

- [ ] **Step 1: Crate headers**

`crates/contrib/pgvector/src/lib.rs` lines 1-4 become:
```rust
//! pgvector (github.com/pgvector/pgvector), ported from 0.8.5 @ 159b79a and
//! brought to 0.8.7 behavior for the vector type: distance/arithmetic
//! functions and aggregates. halfvec/sparsevec/bit and ivfflat are unported;
//! the shipped extension script is still the trimmed vector--0.8.5.sql until
//! M4 (spec §4.6). DIVERGENCE: pg_get_loaded_modules() reports 18.6 for this
//! library; C reports PG_MODULE_MAGIC_EXT's "0.8.7" (vector.c:49). Revisit in M4.
```
In the `pgvector_hnsw/src/lib.rs` header:
- `//! pgvector 0.8.5 hnsw AM` becomes `//! pgvector 0.8.7 hnsw AM`;
- the `level RNG uses the ported pg_global_prng (same generator, per-backend seeding).` sentence becomes `level RNG uses the ported pg_global_prng (same generator, per-backend seeding as stock C; builds reseed only with pgrust.hnsw_build_seed, C's -DHNSW_MEMORY SeedRandom).`

In `pgvector_hnsw_build/src/lib.rs` line 1, `pgvector 0.8.5 hnswbuild.c` becomes `pgvector 0.8.7 hnswbuild.c`.

In `utils.rs:66`, `the trimmed vector--0.8.5.sql` stays as it is: it is still true.

- [ ] **Step 2: `CLAUDE.md`**

In the "Vector search (fork work)" section:
- replace the first sentence with `There is an existing pgvector port (upstream 0.8.5 commit \`159b79a\`, brought to 0.8.7 behavior for \`vector\` and HNSW in M1; the SQL script is still the trimmed 0.8.5 one):`;
- add these lines to the harness command block:
  ```bash
  scripts/pgvector/server.sh {start|fresh|stop|psql} {pgrust|ref}  # manual servers on the harness ports
  scripts/pgvector/docker-ref.sh {up|down|psql}                  # pgvector/pgvector:0.8.7-pg18 reference (port 55493)
  scripts/pgvector/run-bytecmp.sh {pgrust|ref}                    # HNSW pages vs the seeded C build (pgvec-seeded, -DHNSW_MEMORY)
  scripts/pgvector/run-iterscan.sh {pgrust|ref}                   # iterative-scan stop points vs the seeded C build
  ```
- after the block, add: `Stock C pgvector never seeds HNSW builds; \`SET pgrust.hnsw_build_seed = 42\` (an unregistered placeholder option) makes pgrust match the seeded C build. Baselines: \`docs/superpowers/reports/pgvector-m0-baseline.md\`, \`docs/superpowers/reports/pgvector-m1-report.md\`.`

Replace the existing `Baseline: ...` line rather than duplicating it.

- [ ] **Step 3: Full gate**

```bash
scripts/pgvector/build-reference.sh --verify
scripts/pgvector/build-pgrust.sh
scripts/pgvector/run-all.sh ref
scripts/pgvector/tests/harness_test.sh
scripts/pgvector/run-all.sh pgrust || true
UT_M1   # the command spelled out under Global Constraints
bash crates/_support/seams_init/tests/lint-determinism.sh
bash crates/_support/seams_init/tests/lint-seam-installs.sh
git -C crates/pgvector-0.8.7-reference status --porcelain --ignored .; git -C crates/postgres-18.6-reference status --porcelain --ignored .
```
Expected:
- `run-all.sh ref` is all green: 62 regress+TAP, 10 bytecmp, 7 iterscan, so `**79/79 passed**`.
- `harness_test.sh` prints `all passed`.
- In `run-all.sh pgrust`:
  - regress passes `hnsw_vector` and `vector_type`;
  - TAP passes the M0 eleven plus `016`, `043` and `044`;
  - bytecmp is 10/10 and iterscan 7/7;
  - every other failure is one the M0 baseline attributes to M2–M6. Compare the table with the baseline report.
- The unit tests and lints pass, and both reference trees print nothing.

- [ ] **Step 4: Write the M1 report**

Create `docs/superpowers/reports/pgvector-m1-report.md` from the actual Step 3 output:
```markdown
# pgvector conformance after M1

Date: <run date> · pgrust commit: <git rev-parse --short HEAD> · Reference: PostgreSQL 18.6 + pgvector 0.8.7 (`pgvec`), seeded oracle `pgvec-seeded` (-DHNSW_MEMORY)

## Summary
- C reference: <P>/<T> (regress 14/14, TAP 48/48, bytecmp 10/10, iterscan 7/7).
- pgrust: <P>/<T> (regress <p>/14, TAP <p>/48, bytecmp <p>/10, iterscan <p>/7). M0 was 12/62 on regress+TAP.
- Fixed in M1: regress `vector_type`; TAP `016_hnsw_inserts`, `043_hnsw_iterative_scan`, `044_hnsw_iterative_scan_recall`.

## pgrust results
<paste $PGV_WORK/report-pgrust.md's table>

## Remaining failures
Every remaining failure keeps its M0 cause (`pgvector-m0-baseline.md`): types not yet ported (M2/M3), IVFFlat (M4), parallel builds (012, 045: M6). List any test whose cause changed.

## What M1 established
- Stock C pgvector never seeds HNSW builds (`SeedRandom(42)` is under `#ifdef HNSW_MEMORY`); `pgrust.hnsw_build_seed` reproduces the seeded build on demand.
- With seed 42, pgrust's HNSW index pages are byte-identical to C's on tie-free and tie-heavy data, unlogged init forks and post-build inserts.
- Iterative scans stop at exactly C's point (memory cap and tuple limit), through rescans.

## Known divergences left open
- Build memory accounting decides the spill point (`run-bytecmp.sh pgrust spill`: C flushes after 1436 tuples, pgrust after 1840 at 1MB); M6 reworks build memory.
- Cosine/non-integer data: C's `-ffp-contract=fast` fuses multiply-adds (`run-bytecmp.sh pgrust cosine`).
- SelectNeighbors tie-break uses element ids where C uses addresses; they agree while C's graph fits one 1MB block (all tier cases). Unverified beyond that.
- `pg_get_loaded_modules()` version for `vector` (18.6 vs 0.8.7): M4.
- Error-location fields for pre-existing pgvector errors (crate-derived file names; `types_error/src/source_map_table.rs:197` has a malformed `"{hnsw.c"` entry): not addressed in M1.
```
Fill in every `<…>` from the real output. Run the two opt-in cases once with `scripts/pgvector/run-bytecmp.sh pgrust spill cosine || true` so the report states their current numbers.

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md docs/superpowers/reports/pgvector-m1-report.md crates/contrib/pgvector/src/lib.rs crates/contrib/pgvector_hnsw/src/lib.rs crates/contrib/pgvector_hnsw_build/src/lib.rs
git commit -q -F - <<'EOF'
docs(pgvector): M1 report; headers and CLAUDE.md at 0.8.7

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
```

---

## M1 exit criteria (spec §9)

- [ ] Regression `vector_type` and `hnsw_vector` pass on pgrust.
- [ ] TAP `016_hnsw_inserts`, `043_hnsw_iterative_scan` and `044_hnsw_iterative_scan_recall` pass on pgrust. Every HNSW TAP test that passed in M0 still passes.
- [ ] `run-bytecmp.sh pgrust`: 10/10 identical. `run-iterscan.sh pgrust`: 7/7 identical.
- [ ] `run-all.sh ref` is all green, and `harness_test.sh` prints `all passed`.
- [ ] Unit tests and both lint scripts pass. Both reference trees are untouched.
- [ ] `docs/superpowers/reports/pgvector-m1-report.md` is committed. Then merge `vector/m1` into `main` locally (push only on request).
