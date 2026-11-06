//! The graph's memory accounting: C's arithmetic over C's allocation sizes,
//! so a build flushes where C's does. C counts the bytes its allocator holds:
//! the graph's Generation context's blocks in a serial build
//! (HnswMemoryContextAlloc, hnswbuild.c:649-658), the MAXALIGNed bump
//! allocations from the shared area in a parallel build
//! (HnswSharedMemoryAlloc, hnswbuild.c:663-681). All sizes are LP64 PG 18.6.

/// MAXALIGN (pg_config.h MAXIMUM_ALIGNOF 8).
pub const fn maxalign(n: usize) -> usize {
    (n + 7) & !7
}

/// sizeof(HnswElementData) (hnsw.h:182-198); types_hnsw's
/// HNSW_ELEMENT_DATA_SIZE, which the scan-memory model uses too.
pub const ELEMENT_DATA_SIZE: usize = 128;
/// sizeof(HnswNeighborArrayPtr) (hnsw.h:178): a pointer / relptr union.
pub const NEIGHBOR_ARRAY_PTR_SIZE: usize = 8;
/// HNSW_MAX_GRAPH_MEMORY (hnswbuild.c:83): SIZE_MAX / 2.
pub const HNSW_MAX_GRAPH_MEMORY: usize = usize::MAX / 2;

/// HNSW_NEIGHBOR_ARRAY_SIZE(lm) (hnsw.h:84): offsetof(items) = 8, plus 16
/// per HnswCandidate (an element pointer, a float4 distance, a bool closer).
pub const fn neighbor_array_size(lm: i32) -> usize {
    8 + 16 * lm as usize
}

/// HnswGetLayerM (hnsw.h:127).
pub const fn layer_m(m: i32, layer: i32) -> i32 {
    if layer == 0 {
        m * 2
    } else {
        m
    }
}

/// MAXALIGN(sizeof(GenerationContext)): MemoryContextData (80,
/// memnodes.h:117-134), four uint32s, two block pointers and a dlist_head
/// (generation.c:59-72).
const GENERATION_CONTEXT_SIZE: usize = 128;
/// Generation_BLOCKHDRSZ = MAXALIGN(sizeof(GenerationBlock)) (generation.c:46, 87-96).
const GENERATION_BLOCKHDRSZ: usize = 56;
/// Generation_CHUNKHDRSZ = sizeof(MemoryChunk) (generation.c:47).
const GENERATION_CHUNKHDRSZ: usize = 8;
/// generation.c:49.
const GENERATION_CHUNK_FRACTION: usize = 8;
/// memutils_memorychunk.h:96.
const MEMORYCHUNK_MAX_VALUE: usize = 0x3FFF_FFFF;

/// generation.c's block arithmetic, as MemoryContextMemAllocated sees it.
/// Without MEMORY_CONTEXT_CHECKING (the reference and Docker builds have no
/// USE_ASSERT_CHECKING), a chunk is MAXALIGN(size) plus its header. A build
/// frees nothing, so generation.c's freeblock reuse (generation.c:571-587)
/// never applies.
#[derive(Clone, Debug)]
pub struct GenerationModel {
    mem_allocated: usize,
    free: usize,
    next_block_size: usize,
    max_block_size: usize,
    alloc_chunk_limit: usize,
}

impl GenerationModel {
    /// GenerationContextCreate (generation.c:160-271).
    pub fn new(min_context_size: usize, init_block_size: usize, max_block_size: usize) -> GenerationModel {
        // Determine size of initial block
        let mut alloc_size = GENERATION_CONTEXT_SIZE + GENERATION_BLOCKHDRSZ + GENERATION_CHUNKHDRSZ;
        alloc_size = if min_context_size != 0 {
            alloc_size.max(min_context_size)
        } else {
            alloc_size.max(init_block_size)
        };
        // The keeper block follows the context header in the same malloc.
        let first_block_size = alloc_size - GENERATION_CONTEXT_SIZE;
        let mut alloc_chunk_limit = max_block_size.min(MEMORYCHUNK_MAX_VALUE);
        while alloc_chunk_limit + GENERATION_CHUNKHDRSZ
            > (max_block_size - GENERATION_BLOCKHDRSZ) / GENERATION_CHUNK_FRACTION
        {
            alloc_chunk_limit >>= 1;
        }
        GenerationModel {
            mem_allocated: first_block_size,
            free: first_block_size - GENERATION_BLOCKHDRSZ,
            next_block_size: init_block_size,
            max_block_size,
            alloc_chunk_limit,
        }
    }

    /// MemoryContextMemAllocated(context, false).
    pub fn mem_allocated(&self) -> usize {
        self.mem_allocated
    }

    /// GenerationAlloc (generation.c:527-600).
    pub fn alloc(&mut self, size: usize) {
        let chunk_size = maxalign(size);
        if chunk_size > self.alloc_chunk_limit {
            // GenerationAllocLarge (generation.c:343-372): a dedicated block.
            self.mem_allocated += chunk_size + GENERATION_CHUNKHDRSZ + GENERATION_BLOCKHDRSZ;
            return;
        }
        let required_size = chunk_size + GENERATION_CHUNKHDRSZ;
        if self.free < required_size {
            // GenerationAllocFromNewBlock (generation.c:461-500): the first
            // block has initBlockSize, doubling up to maxBlockSize, rounded
            // up to a power of 2 if the chunk needs more.
            let mut blksize = self.next_block_size;
            self.next_block_size = (self.next_block_size << 1).min(self.max_block_size);
            let need = chunk_size + GENERATION_CHUNKHDRSZ + GENERATION_BLOCKHDRSZ;
            if blksize < need {
                blksize = need.next_power_of_two();
            }
            self.mem_allocated += blksize;
            self.free = blksize - GENERATION_BLOCKHDRSZ;
        }
        self.free -= required_size;
    }
}

/// HnswSharedMemoryAlloc's errors (hnswbuild.c:671-676). The 1 MB margin
/// keeps them unreachable for any value HNSW can index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphError {
    /// "hnsw allocation too large"
    AllocationTooLarge,
    /// "hnsw allocator out of memory"
    OutOfMemory,
    /// The host's allocator refused `bytes` for something C pallocs in
    /// `context`: palloc's "out of memory" ERROR (mcxt.c:1163-1167), where
    /// an infallible Rust allocation would abort the server.
    HostOutOfMemory { context: &'static str, bytes: usize },
}

#[derive(Clone, Debug)]
enum Mode {
    Serial(GenerationModel),
    Parallel,
}

/// The graph's memoryUsed and memoryTotal (hnsw.h:245-247) and the allocator
/// behind them.
#[derive(Clone, Debug)]
pub struct Accounting {
    mode: Mode,
    used: usize,
    total: usize,
}

/// In a parallel build, add a margin so allocations never fail (hnswbuild.c:509-510).
const PARALLEL_MEMORY_MARGIN: usize = 1024 * 1024;
/// HnswSharedMemoryAlloc's largest chunk (hnswbuild.c:671).
const MAX_SHARED_ALLOCATION: usize = 1024 * 1024;

impl Accounting {
    /// A serial build's graph: InitGraph(&graphData, NULL,
    /// maintenance_work_mem * 1024) (hnswbuild.c:727) over the 1 MB-block
    /// Generation context (hnswbuild.c:732-737).
    pub fn serial(memory_total: usize) -> Accounting {
        Accounting {
            mode: Mode::Serial(GenerationModel::new(1024 * 1024, 1024 * 1024, 1024 * 1024)),
            used: 0,
            total: memory_total.min(HNSW_MAX_GRAPH_MEMORY),
        }
    }

    /// A parallel build's graph: InitGraph(&hnswshared->graphData, hnswarea,
    /// esthnswarea) (hnswbuild.c:1012-1013).
    pub fn parallel(area_size: usize) -> Accounting {
        Accounting { mode: Mode::Parallel, used: 0, total: area_size.min(HNSW_MAX_GRAPH_MEMORY) }
    }

    /// esthnswarea (hnswbuild.c:961-969): leave space for other objects in
    /// shared memory; maintenance_work_mem is in kB.
    pub fn parallel_area_size(maintenance_work_mem: i32) -> usize {
        let mut esthnswarea = (maintenance_work_mem.max(0) as usize).saturating_mul(1024);
        let estother = 3 * 1024 * 1024;
        if esthnswarea > estother {
            esthnswarea -= estother;
        }
        esthnswarea.min(HNSW_MAX_GRAPH_MEMORY)
    }

    /// memoryUsed.
    pub fn used(&self) -> usize {
        self.used
    }

    /// memoryTotal.
    pub fn total(&self) -> usize {
        self.total
    }

    /// InsertTuple's flush test, `add_size(memoryUsed, memoryMargin) >=
    /// memoryTotal` (hnswbuild.c:533); the margin is 0 in a serial build.
    pub fn is_full(&self) -> bool {
        let margin = match self.mode {
            Mode::Serial(_) => 0,
            Mode::Parallel => PARALLEL_MEMORY_MARGIN,
        };
        self.used.saturating_add(margin) >= self.total
    }

    /// One allocation from the graph's allocator.
    fn alloc(&mut self, size: usize) -> Result<(), GraphError> {
        match &mut self.mode {
            // HnswMemoryContextAlloc: memoryUsed is the context's block total.
            Mode::Serial(generation) => {
                generation.alloc(size);
                self.used = generation.mem_allocated();
                Ok(())
            }
            // HnswSharedMemoryAlloc: a bump of MAXALIGN(size).
            Mode::Parallel => {
                let aligned_size = maxalign(size);
                if aligned_size > MAX_SHARED_ALLOCATION {
                    return Err(GraphError::AllocationTooLarge);
                }
                let new_memory_used = self.used + aligned_size;
                if new_memory_used > self.total {
                    return Err(GraphError::OutOfMemory);
                }
                self.used = new_memory_used;
                Ok(())
            }
        }
    }

    /// HnswInitElement's allocations and the value's (hnswutils.c:205-270,
    /// hnswbuild.c:556-557), in C's order: the element, its neighbor-array
    /// pointers, one neighbor array per layer from 0 to `level`, the value.
    pub fn charge_element(&mut self, level: u8, m: i32, value_len: usize) -> Result<(), GraphError> {
        self.alloc(ELEMENT_DATA_SIZE)?;
        self.alloc(NEIGHBOR_ARRAY_PTR_SIZE * (level as usize + 1))?;
        for lc in 0..=level as i32 {
            self.alloc(neighbor_array_size(layer_m(m, lc)))?;
        }
        self.alloc(value_len)
    }
}
