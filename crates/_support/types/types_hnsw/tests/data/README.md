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
