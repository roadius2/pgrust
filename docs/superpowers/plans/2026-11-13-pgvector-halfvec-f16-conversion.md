# pgvector: the `halfvec` f16→f32 conversion — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring pgrust's `halfvec` index builds from 3.9× to 11.2× C down to the neighbourhood of its own `vector` builds (1.5× to 1.7× C) by replacing the per-element software f16→f32 conversion inside the four `halfvec` distance kernels with a block conversion that is vectorized, while every result stays bit-identical.

**Architecture:** `crates/contrib/pgvector/src/halfutils.rs` gains `halves_to_floats`, which converts a block of native-endian halves to `f32` with NEON's `fcvtl` on little-endian AArch64 and with a branch-free integer routine LLVM vectorizes everywhere else. The four kernels (`halfvec_l2_squared_distance`, `halfvec_inner_product`, `halfvec_cosine_similarity`, `halfvec_l1_distance`) walk their operands in blocks of 32 halves through one shared `for_each_pair`: convert both blocks into two stack arrays, then accumulate element by element in C source order, exactly as today. The existing `half_to_float4` stays as the per-element reference and for the callers outside the kernels. f16→f32 widening is exact, so a correct conversion yields identical bits; the proof is exhaustive (every half through every path) plus the existing C parity crate, bytecmp's identical pages and the recall tier.

**Tech Stack:** Rust 1.96.0 (pinned), `core::arch::aarch64` NEON intrinsics (stable on 1.96: `vcvt_f32_f16`, `vcvt_high_f32_f16`, `vreinterpret_f16_u16`, `vreinterpretq_f16_u16`), the `scripts/pgvector/` harness, C reference under `~/.cache/pgrust/pgref-18.6/`.

**Spec:** No separate design document: this is a small change, described in chat on 2026-11-13 and approved as "go ahead with your approach". The problem statement is `docs/superpowers/reports/pgvector-speed-2026-11-08.md` ("The `halfvec` lead") and the profile in `~/.cache/pgrust/speed-2026-11-08/profile/` (74% of the HNSW `halfvec` build's samples and 82% of the IVFFlat one's in the conversion iterator). The design is this plan's "Decisions" list.

**Decisions this plan makes:**
1. **Conversion only.** The kernels' accumulation (`distance += diff * diff` and the like) keeps C source order with no reassociation and no fusion, as the user ruled on 2026-10-06. Only where the `f32` operands come from changes.
2. **Block size 32 halves** (`const BLOCK: usize = 32`): two `[f32; 32]` on the stack per kernel call, four NEON steps of eight per operand per block, dim 100 is three blocks and a four-wide tail.
3. **Two lane routes, one function.** `halves_to_floats(src, dst)` uses NEON (`vld1q_u8` → `vreinterpretq_u16_u8` → `vcvt_f32_f16`/`vcvt_high_f32_f16` → `vst1q_f32`) eight halves at a time under `cfg(all(target_arch = "aarch64", target_endian = "little", target_feature = "neon"))`, then `half_to_float4_portable` for the tail; every other target runs `half_to_float4_portable` over the whole block. The byte load keeps the function alignment-free, as `from_ne_bytes` is today.
4. **`half_to_float4_portable`** is the branch-free lane routine: sign `(h & 0x8000) << 16`; normal `((h & 0x7FFF) << 13) + (112 << 23)`; subnormal and zero by the exact magic subtraction `f32::from_bits(0x3880_0000 | (m << 13)) - f32::from_bits(0x3880_0000)` (Sterbenz: both operands within a factor of two, so the difference `m * 2^-24` is exact); infinity and NaN `0x7F80_0000 | (m << 13) | (if m != 0 { 0x0040_0000 } else { 0 })`, which quiets a NaN the way the software path's `0x7FC0_0000` does. Selected by `h & 0x7C00` being `0`, `0x7C00` or neither. The spike (`~/.cache/pgrust/speed-2026-11-08/profile/h2f-spike/`, a copy of the scratchpad crate) found it bit-identical to `half_to_float4` on all 65,536 inputs and LLVM vectorizes it (`cmeq.4s`, `bsl.16b`, `fadd.4s`).
5. **No x86-64 F16C path.** `_mm_cvtph_ps` needs a runtime feature check; the portable route already vectorizes with SSE2. Out of scope.
6. **`half_to_float4` is unchanged** and stays `pub`: it is the reference every test compares against, and `halfvec.rs`, `sparsevec.rs`, `funcs.rs` and `pgvector_ivfflat` keep calling it per element off the hot path (`fc_halfvec_l2_normalize` and the k-means center sums were 0.3% and 0.4% of the IVFFlat build's samples).
7. **Measured on the recall tier at gate size,** release binary, quiet machine, against the 2026-11-08 numbers in the same report, and proven result-neutral by the three `halfvec` bytecmp cases (pages identical to the seeded C build) and the four `halfvec` recall rows.

**Spike numbers (2026-11-13, M5 Max, release profile, inner product at dim 100, best of 5 runs of 200,000 calls):** f32 `vector` kernel 21 ns; `halfvec` today 178 ns; portable block 82 ns; NEON block 34 ns.

**Branch:** `vector/halfvec-f16` off `main` at `6aa3a0be77`, in the main checkout. Commits end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>` and, as the fork's commits do, a line naming the implementing model.

## Global Constraints

- Rust 1.96.0 as pinned by `rust-toolchain.toml`; no new dependencies; no nightly features (`f16` is unstable and is not used).
- Every `unsafe` block carries a `// SAFETY:` comment, as the crate's do.
- The kernels' summation order and the absence of fused multiply-add are unchanged (user decision 2026-10-06).
- `half_to_float4`, `float4_to_half_unchecked` and `float4_to_half` keep their names, signatures and bodies.
- No new infallible run-time-sized allocation in `crates/contrib` (`lint_alloc`); the block buffers are fixed-size stack arrays.
- Timing numbers come only from `release` or `dist` binaries, on AC power with Time Machine idle (`tmutil status` shows `Running = 0`), and are medians of at least three builds.
- Comments cite the C file and lines they port (`halfutils.h`, `halfutils.c`) and mark intentional differences `DIVERGENCE`, as the crate does.

## Review Focus

1. A source slice at an odd byte offset (a `halfvec` payload inside a tuple need not be 2-byte aligned from Rust's point of view): the conversion must read it without an alignment fault and give the same bits. Pinned by `block_conversion_is_alignment_free` in Task 1.
2. Operand slices longer than `2 * dim` (the kernels get `xs()` of a varlena whose trailing bytes may exceed the dimension on a mis-sized input): the kernels must use exactly `dim` halves, as `x[..2 * dim]` does today. Pinned by `kernels_ignore_bytes_past_dim` in Task 2.
3. Dimensions at every residue modulo 32 and modulo 8, including exact multiples (32, 64) with no tail and the largest `halfvec` dimension 16000: no lane dropped, no lane doubled. Pinned by `kernels_match_per_element_reference` (dims 1 to 100) and `kernels_at_max_dim` in Task 2.
4. An empty destination (`dst.len() == 0`) and a source exactly `2 * dst.len()` long: no panic, nothing written. Pinned by `block_conversion_of_nothing` in Task 1.
5. NaN and infinity halves, which the kernels never see but `halves_to_floats` may be given: identical bits to `half_to_float4`, payload and quiet bit included. Pinned by the exhaustive test in Task 1 (every half is a case).

---

### Task 1: the block conversion

**Files:**
- Modify: `crates/contrib/pgvector/src/halfutils.rs` (module doc lines 1-12; add after `half_to_float4`, which ends at line 73; tests module from line 190)
- Modify: `crates/contrib/pgvector_f16_parity/src/lib.rs:41-53`

**Interfaces:**
- Consumes: `half_to_float4(num: Half) -> f32` (`halfutils.rs:39`), `Half = u16`.
- Produces:
  - `pub fn half_to_float4_portable(num: Half) -> f32` — branch-free, bit-identical to `half_to_float4` for every input.
  - `pub fn halves_to_floats(src: &[u8], dst: &mut [f32])` — writes `dst[i] = half_to_float4(Half::from_ne_bytes([src[2*i], src[2*i+1]]))` for every `i < dst.len()`; reads only `src[..2 * dst.len()]` and panics if `src` is shorter (the one bounds check, as the kernels' `x[..2 * dim]` does today).
  - `pub const BLOCK: usize = 32` — the block the kernels use (Task 2).

- [ ] **Step 1: Write the failing tests** in `halfutils.rs`'s `tests` module:

```rust
// Every half through both lane routes: the portable routine on its own, and
// the block conversion at every position of a 37-wide block, so on AArch64
// each half crosses the NEON lanes 0..31 and the portable tail 32..36.
#[test]
fn block_conversion_matches_per_element_for_every_half() {
    const W: usize = 37;
    let mut src = [0u8; 2 * W];
    let mut dst = [1.5f32; W];
    for h in 0..=u16::MAX {
        let want = half_to_float4(h).to_bits();
        assert_eq!(half_to_float4_portable(h).to_bits(), want, "portable, half {h:#06x}");
        for pos in 0..W {
            src.fill(0);
            src[2 * pos..2 * pos + 2].copy_from_slice(&h.to_ne_bytes());
            halves_to_floats(&src, &mut dst);
            assert_eq!(dst[pos].to_bits(), want, "block, half {h:#06x} at {pos}");
            assert!(dst.iter().enumerate().all(|(i, f)| i == pos || f.to_bits() == 0), "zeros elsewhere, half {h:#06x} at {pos}");
        }
    }
}

// The source may start at any byte: the same 100 halves converted from an
// even and from an odd offset of one buffer.
#[test]
fn block_conversion_is_alignment_free() {
    let mut r = XorShift(0x2545_F491_4F6C_DD1D);
    let halves: Vec<Half> = (0..100).map(|_| r.half()).collect();
    let bytes: Vec<u8> = halves.iter().flat_map(|h| h.to_ne_bytes()).collect();
    let mut shifted = vec![0u8; 1];
    shifted.extend_from_slice(&bytes);
    let (mut even, mut odd) = (vec![0f32; 100], vec![0f32; 100]);
    halves_to_floats(&bytes, &mut even);
    halves_to_floats(&shifted[1..], &mut odd);
    for i in 0..100 {
        assert_eq!(even[i].to_bits(), half_to_float4(halves[i]).to_bits());
        assert_eq!(odd[i].to_bits(), even[i].to_bits());
    }
}

// Nothing to convert, and a source exactly as long as the destination needs.
#[test]
fn block_conversion_of_nothing() {
    let mut none: [f32; 0] = [];
    halves_to_floats(&[], &mut none);
    halves_to_floats(&[1, 2, 3, 4], &mut none);
    let mut two = [9.0f32; 2];
    halves_to_floats(&0x3C00u16.to_ne_bytes().iter().chain(&0x4000u16.to_ne_bytes()).copied().collect::<Vec<u8>>(), &mut two);
    assert_eq!(two, [1.0, 2.0]);
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p pgvector --lib halfutils::tests 2>&1 | tail -20`
Expected: compile error, `cannot find function halves_to_floats` (and `half_to_float4_portable`).

- [ ] **Step 3: Implement** in `halfutils.rs`, after `half_to_float4`:

- `pub const BLOCK: usize = 32;` with a one-line doc saying what it is for.
- `pub fn half_to_float4_portable(num: Half) -> f32`, `#[inline(always)]`, the routine of Decision 4. Its comment cites `halfutils.h:62-141` as the behaviour it reproduces and states the exactness argument in one line each for the three cases.
- `pub fn halves_to_floats(src: &[u8], dst: &mut [f32])`, `#[inline]`: `let src = &src[..2 * dst.len()];` first; then under the NEON `cfg` of Decision 3, `src.chunks_exact(16)` zipped with `dst.chunks_exact_mut(8)` through the intrinsics, and the two remainders through a private `fn portable_block(src: &[u8], dst: &mut [f32])` that loops `half_to_float4_portable` over `dst.iter_mut().zip(src.chunks_exact(2))`; without NEON, `portable_block` over everything. The `unsafe` block holds only the four intrinsic calls and the two stores, with a `// SAFETY:` comment naming the lengths (16 readable bytes in, eight writable `f32` out; `vld1q_u8` takes a byte-aligned pointer, `vst1q_f32` an `f32`-aligned one, which `&mut [f32]` gives).
- Module doc (lines 1-12): add that the kernels convert in blocks through `halves_to_floats` (NEON `fcvtl` on little-endian AArch64, a vectorizable branch-free routine elsewhere), that `half_to_float4` remains the per-element software port, and that every route is bit-identical (exhaustive test here; the C parity crate).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p pgvector --lib halfutils::tests 2>&1 | tail -20`
Expected: all `halfutils::tests` pass, the three new ones included.

- [ ] **Step 5: Extend the C parity test** in `pgvector_f16_parity/src/lib.rs`: inside `f16_half_to_float4_matches_c_for_every_half`'s loop, after the two existing assertions, convert `h` through `halves_to_floats` at position `h as usize % 9` of a 9-wide block (so on AArch64 the NEON lanes and the tail both meet C) and assert the bits equal `pgv_sw_half_to_float4(h)`, and `pgv_hw_half_to_float4(h)` when `hw`. Import `halves_to_floats` beside `half_to_float4`.

- [ ] **Step 6: Run the parity crate**

Run: `cargo test -p pgvector_f16_parity --release 2>&1 | tail -8`
Expected: 3 tests pass (`f16_float4_to_half_matches_c_for_every_float` walks 2^32 inputs; release keeps that to seconds).

- [ ] **Step 7: Lint**

Run: `cargo clippy -p pgvector -p pgvector_f16_parity --no-deps -- -D warnings 2>&1 | tail -5` and `cargo fmt -p pgvector -p pgvector_f16_parity -- --check`
Expected: no output but the `Finished` line; fmt prints nothing.

- [ ] **Step 8: Commit**

```bash
git add crates/contrib/pgvector/src/halfutils.rs crates/contrib/pgvector_f16_parity/src/lib.rs
git commit -m "pgvector: a block f16->f32 conversion, NEON fcvtl or branch-free, bit-identical to the software path"
```

---

### Task 2: the kernels convert in blocks

**Files:**
- Modify: `crates/contrib/pgvector/src/halfutils.rs:141-188` (`half_floats` and the four kernels) and its tests (`kernels_match_per_element_reference`, line 215)

**Interfaces:**
- Consumes: `halves_to_floats`, `BLOCK` (Task 1).
- Produces: the four kernels with unchanged signatures: `pub fn halfvec_l2_squared_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32`, `pub fn halfvec_inner_product(dim: usize, ax: &[u8], bx: &[u8]) -> f32`, `pub fn halfvec_cosine_similarity(dim: usize, ax: &[u8], bx: &[u8]) -> f64`, `pub fn halfvec_l1_distance(dim: usize, ax: &[u8], bx: &[u8]) -> f32`. Private `fn for_each_pair(dim: usize, ax: &[u8], bx: &[u8], f: impl FnMut(f32, f32))`.

- [ ] **Step 1: Extend and add tests** in the `tests` module:

- In `kernels_match_per_element_reference`, change `let dim = 1 + n % 40;` to `let dim = 1 + n % 100;` so dims cover 1 to 100: every residue modulo 32 and 8, the exact multiples 32, 64 and 96, and three full blocks plus a tail.
- Add:

```rust
// The kernels read exactly dim halves: trailing bytes past 2*dim change nothing.
#[test]
fn kernels_ignore_bytes_past_dim() {
    let mut r = XorShift(0x0123_4567_89AB_CDEF);
    for &dim in &[1usize, 7, 32, 33, 100] {
        let a: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
        let b: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
        let (mut a2, mut b2) = (a.clone(), b.clone());
        a2.extend_from_slice(&[0xFF; 70]);
        b2.extend_from_slice(&[0x7C, 0x7C, 0x01]);
        assert_eq!(halfvec_l2_squared_distance(dim, &a, &b).to_bits(), halfvec_l2_squared_distance(dim, &a2, &b2).to_bits());
        assert_eq!(halfvec_inner_product(dim, &a, &b).to_bits(), halfvec_inner_product(dim, &a2, &b2).to_bits());
        assert_eq!(halfvec_cosine_similarity(dim, &a, &b).to_bits(), halfvec_cosine_similarity(dim, &a2, &b2).to_bits());
        assert_eq!(halfvec_l1_distance(dim, &a, &b).to_bits(), halfvec_l1_distance(dim, &a2, &b2).to_bits());
    }
}

// HALFVEC_MAX_DIM (halfvec.h:9) is 16000: 500 full blocks, against the
// per-element form.
#[test]
fn kernels_at_max_dim() {
    let dim = 16000usize;
    let mut r = XorShift(0xDEAD_BEEF_CAFE_F00D);
    let a: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
    let b: Vec<u8> = (0..dim).flat_map(|_| r.half().to_ne_bytes()).collect();
    let hx = |x: &[u8], i: usize| half_to_float4(Half::from_ne_bytes([x[2 * i], x[2 * i + 1]]));
    let mut ip = 0.0f32;
    for i in 0..dim {
        ip += hx(&a, i) * hx(&b, i);
    }
    assert_eq!(halfvec_inner_product(dim, &a, &b).to_bits(), ip.to_bits());
}
```

- [ ] **Step 2: Run them; the new ones pass trivially against the old kernels, so confirm they compile and run**

Run: `cargo test -p pgvector --lib halfutils::tests 2>&1 | tail -12`
Expected: all pass (these tests pin behaviour the rewrite must keep).

- [ ] **Step 3: Rewrite the kernels** in `halfutils.rs:141-188`:

- Replace `half_floats` with `fn for_each_pair(dim: usize, ax: &[u8], bx: &[u8], mut f: impl FnMut(f32, f32))`, `#[inline]`: slice `ax[..2 * dim]` and `bx[..2 * dim]` once; `let (mut fa, mut fb) = ([0f32; BLOCK], [0f32; BLOCK]);`; iterate `ax.chunks(2 * BLOCK).zip(bx.chunks(2 * BLOCK))`, per pair `let n = ca.len() / 2;`, `halves_to_floats(ca, &mut fa[..n]); halves_to_floats(cb, &mut fb[..n]);`, then `for (a, b) in fa[..n].iter().zip(&fb[..n]) { f(*a, *b) }`. Its comment: the conversion is vectorized per block, the accumulation stays element by element in C source order (halfutils.c's `*Default` loops), and why (the 2026-10-06 ruling; the module doc).
- Each kernel keeps its accumulator declarations and its final expression, and replaces its `for` loop with one `for_each_pair(dim, ax, bx, |a, b| { ... })` whose body is the loop body it had (the cosine kernel's closure updates its three accumulators; keep the variable names `axi`, `bxi`, `similarity`, `norma`, `normb`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p pgvector 2>&1 | tail -12`
Expected: every `pgvector` test passes (the whole crate, since `halfvec.rs`'s tests exercise the kernels through the SQL functions).

- [ ] **Step 5: Lint and format**

Run: `cargo clippy -p pgvector --no-deps -- -D warnings 2>&1 | tail -5 && cargo fmt -p pgvector -- --check`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/contrib/pgvector/src/halfutils.rs
git commit -m "pgvector: the halfvec kernels convert f16 in blocks; accumulation order unchanged"
```

---

### Task 3: proof and measurement

**Files:**
- Modify: `docs/superpowers/reports/pgvector-speed-2026-11-08.md` (append a section)
- Modify: `CLAUDE.md` (the "Speed (2026-11-08 …)" paragraph in "Vector search (fork work)")
- Evidence: `~/.cache/pgrust/speed-2026-11-08/halfvec-fix/` (outside the repo)

**Interfaces:**
- Consumes: the committed Tasks 1 and 2; `scripts/pgvector/common.sh`'s `PGRUST_PROFILE` (default `fast-profile`; `release` for timings); the 2026-11-08 numbers in the report's table C: `hnsw_halfvec_serial` C 9100.2 ms / pgrust 39929.0; `hnsw_halfvec_parallel` 3403.7 / 13202.6; `ivfflat_halfvec_serial` 768.2 / 8583.0; `ivfflat_halfvec_parallel` 618.1 / 4984.2; `hnsw_vector_serial` 10268.4 / 17005.3; `ivfflat_vector_serial` 784.1 / 1764.7.
- Produces: the report section and the CLAUDE.md sentence other sessions read.

- [ ] **Step 1: Unit tiers and lints on the branch**

Run, from the repo root:
```bash
cargo test -p pgvector -p pgvector_f16_parity --release 2>&1 | grep -E "^test result|FAILED|panicked"
cargo clippy -p pgvector -p pgvector_f16_parity --no-deps -- -D warnings 2>&1 | tail -3
cargo test -p seams_init --test lint_alloc 2>&1 | grep -E "^test result|FAILED"
```
Expected: every `test result:` line `ok`, no `FAILED`; clippy clean; `lint_alloc` ok. Save the output to `~/.cache/pgrust/speed-2026-11-08/halfvec-fix/unit.log`.

- [ ] **Step 2: Build both binaries**

Run: `scripts/pgvector/build-pgrust.sh 2>&1 | tail -2` (fast-profile, for the bytecmp and regress tiers) and `cargo build --release --locked -p main_main -p initdb 2>&1 | tail -2` (about 90 s each).
Expected: both end with `Finished`.

- [ ] **Step 3: Pages identical to the seeded C build**

Run: `scripts/pgvector/run-bytecmp.sh pgrust halfvec_l2_grid halfvec_cosine_grid ivf_halfvec_groups 2>&1 | tail -6`
Expected: 3/3 cases identical. These compare pgrust's HNSW and IVFFlat pages byte for byte with C's seeded build, so any distance that changed would show. Copy `~/.cache/pgrust/pgvector-work/bytecmp/pgrust/summary.tsv` to the evidence directory as `bytecmp-summary.tsv`.

- [ ] **Step 4: The halfvec regression tests**

Run: `scripts/pgvector/run-regress.sh pgrust halfvec hnsw_halfvec ivfflat_halfvec cast btree copy 2>&1 | tail -4`
Expected: `6 tests passed` (or the script's equivalent all-ok line); no diffs under `~/.cache/pgrust/pgvector-work/regress/pgrust`.

- [ ] **Step 5: Timing on a quiet machine**

Check first: `pmset -g batt | head -1` says `AC Power`; `tmutil status | grep -c "Running = 1"` prints `0` (if a backup is running, wait for it, as the 2026-11-08 driver did). Then:
```bash
PGRUST_PROFILE=release scripts/pgvector/run-recall.sh pgrust hnsw_halfvec_serial hnsw_halfvec_parallel ivfflat_halfvec_serial ivfflat_halfvec_parallel hnsw_vector_serial ivfflat_vector_serial 2>&1 | tail -15
```
Expected: exit 0, every row ok (8 recall rows and `exact_halfvec`, `exact_vector`). Then copy `~/.cache/pgrust/pgvector-work/recall/pgrust/{recall.tsv,summary.tsv,report.md}` to the evidence directory, and compute each case's median build ms per side from `recall.tsv` (columns: case, side, build number, build ms, ...). Record `pmset`, `tmutil status`, `uptime` and `git rev-parse HEAD` beside them, and `target/release/postgres`'s `version()` string.

Expected magnitude (from the spike, not a pass rule): `hnsw_halfvec_serial` near `hnsw_vector_serial`'s 17 s rather than 40 s; `ivfflat_halfvec_serial` near 2 s rather than 8.6 s.

- [ ] **Step 6: Copy the spike** `/private/tmp/claude-501/-Users-jody-dev-projects-pgrust/03ca059c-a039-408f-bf6e-a5d5ee6795ac/scratchpad/h2f-spike/{Cargo.toml,src/main.rs}` to `~/.cache/pgrust/speed-2026-11-08/profile/h2f-spike/` (Decision 4 cites it there).

- [ ] **Step 7: The report section.** Append to `docs/superpowers/reports/pgvector-speed-2026-11-08.md`:

`## 2026-11-13: the halfvec conversion, fixed` with: one paragraph on what changed (Decisions 1 to 4 in prose, with `halfutils.rs` line references of the new code); the profile that confirmed the lead (74% / 82% of samples in the conversion iterator, `~/.cache/pgrust/speed-2026-11-08/profile/`); the spike's four numbers; a table with columns `case | C (ms) | pgrust 2026-11-08 (ms) | pgrust now (ms) | now ÷ C | now ÷ own vector` for the six cases of Step 5 (the last column only for the `halfvec` rows, against the same run's `vector` row of the same AM and mode; the parallel rows against the 2026-11-08 `vector` parallel rows, say so); the proofs in one list (exhaustive unit tests, the C parity crate, bytecmp 3/3 identical, the recall rows with their values, regress 6/6); limits (one machine, AArch64 only timed, the portable route only unit-tested here); evidence path `~/.cache/pgrust/speed-2026-11-08/halfvec-fix/`. Update the report's summary at the top with one sentence pointing to the section, and strike or annotate the "halfvec lead" paragraph's "not yet a profile" wording.

- [ ] **Step 8: CLAUDE.md.** In the "Speed (2026-11-08 …)" paragraph, replace the clause from "`halfvec` builds are 3.9× to 11.2× C" through "where C's `_Float16` is a hardware widening" with the measured post-fix ratios and one sentence: the kernels convert f16 in blocks (`halves_to_floats`: NEON `fcvtl` on AArch64, a vectorizable branch-free routine elsewhere; `half_to_float4` stays the per-element reference), with the exhaustive tests and `pgvector_f16_parity` as the exactness proof, and point to the report's 2026-11-13 section.

- [ ] **Step 9: Commit**

```bash
git add docs/superpowers/reports/pgvector-speed-2026-11-08.md CLAUDE.md
git commit -m "docs(speed): the halfvec conversion fixed; builds remeasured against C and the 2026-11-08 run"
```

---

## Self-review notes

- Coverage: the problem statement (conversion dominates `halfvec` kernels) is answered by Tasks 1 and 2; the exactness claim by Task 1's exhaustive tests, the parity crate and Task 3's bytecmp; the speed claim by Task 3's timing; the user's standing constraint (summation order) by Task 2's structure and tests, which compare against the per-element form bit for bit.
- Names used across tasks: `halves_to_floats`, `half_to_float4_portable`, `BLOCK`, `for_each_pair`, the four kernel names; all defined where first used.
- Review Focus items 1, 4 and 5 are pinned in Task 1; items 2 and 3 in Task 2.
