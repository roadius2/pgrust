//! ivfkmeans.c (pgvector 0.8.7): k-means++ initialization, Elkan's k-means
//! over the sampled vectors (spherical for inner product and cosine), then
//! the center checks.

use datum::Datum;
use pgvector_ivfflat::utils::{
    elog_error, index_collation, index_getprocinfo, ivfflat_check_memory_usage, ivfflat_norm_vectors,
    ivfflat_optional_proc_info, vector_array_size, VectorArray,
};
use types_core::Oid;
use types_error::PgResult;
use types_fmgr::FmgrInfo;
use types_ivfflat::*;
use types_rel::Relation;

// The support procs ivfkmeans.c fetches from the index: the k-means distance
// (proc 3; InitCenters and ElkanKmeans, reached only when there are samples),
// the optional k-means norm (proc 4) and the optional norm (proc 2,
// CheckNorms). Fetched once up front instead of in each callee.
pub struct KmeansProcs {
    pub collation: Oid,
    pub distance: Option<FmgrInfo>,
    pub kmeans_norm: Option<FmgrInfo>,
    pub norm: Option<FmgrInfo>,
}

// RandomDouble() and RandomInt() (ivfflat.h:84-86): pg_global_prng_state.
fn random_double() -> f64 {
    pg_prng::global_prng(|p| p.next_f64())
}

fn random_int() -> u32 {
    pg_prng::global_prng(|p| p.next_u32())
}

// DatumGetFloat8(FunctionCall2Coll(procinfo, collation, a, b)).
fn distance(procinfo: &mut FmgrInfo, collation: Oid, a: Datum, b: Datum) -> PgResult<f64> {
    Ok(types_fmgr::function_call2_coll(procinfo, collation, a, b)?.as_f64())
}

// InitCenters (ivfkmeans.c:23-91): k-means++.
fn init_centers(
    procinfo: &mut FmgrInfo,
    collation: Oid,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    lower_bound: &mut [f32],
) -> PgResult<()> {
    let num_centers = centers.maxlen;
    let num_samples = samples.length;
    let nc = num_centers as usize;
    let mut weight = vec![f32::MAX; num_samples as usize];

    // Choose an initial center uniformly at random
    let first = (random_int() % samples.length as u32) as i32;
    centers.set(0, samples.get(first)?)?;
    centers.length += 1;

    for i in 0..num_centers {
        postgres_seams::check_for_interrupts::call()?;

        let mut sum = 0.0f64;
        for j in 0..num_samples {
            let vec = samples.datum(j)?;
            // Only need to compute distance for new center
            let mut d = distance(procinfo, collation, vec, centers.datum(i)?)?;
            // Set lower bound
            lower_bound[j as usize * nc + i as usize] = d as f32;
            // Use distance squared for weighted probability distribution
            d *= d;
            if d < weight[j as usize] as f64 {
                weight[j as usize] = d as f32;
            }
            sum += weight[j as usize] as f64;
        }

        // Only compute lower bound on last iteration
        if i + 1 == num_centers {
            break;
        }

        // Choose new center using weighted probability distribution.
        let mut choice = sum * random_double();
        let mut j = 0;
        while j < num_samples - 1 {
            choice -= weight[j as usize] as f64;
            if choice <= 0.0 {
                break;
            }
            j += 1;
        }

        centers.set(i + 1, samples.get(j)?)?;
        centers.length += 1;
    }
    Ok(())
}

// NormCenters (ivfkmeans.c:96-105).
fn norm_centers(type_info: &IvfflatTypeInfo, collation: Oid, centers: &mut VectorArray<'_>) -> PgResult<()> {
    ivfflat_norm_vectors(type_info, collation, centers)
}

// RandomCenters (ivfkmeans.c:110-133): quick approach if we have no data.
fn random_centers(procs: &KmeansProcs, centers: &mut VectorArray<'_>, type_info: &IvfflatTypeInfo) -> PgResult<()> {
    let dimensions = centers.dim;
    let mut x = vec![0.0f32; dimensions as usize];

    // Fill with random data
    while centers.length < centers.maxlen {
        for xi in x.iter_mut() {
            *xi = random_double() as f32;
        }
        let center = centers.get_mut(centers.length)?;
        (type_info.update_center)(center, dimensions, &x);
        centers.length += 1;
    }

    if procs.kmeans_norm.is_some() {
        norm_centers(type_info, procs.collation, centers)?;
    }
    Ok(())
}

// SumCenters (ivfkmeans.c:151-160).
fn sum_centers(
    samples: &VectorArray<'_>,
    agg: &mut [f32],
    closest_centers: &[i32],
    type_info: &IvfflatTypeInfo,
) -> PgResult<()> {
    let dim = samples.dim as usize;
    for i in 0..samples.length {
        let c = closest_centers[i as usize] as usize;
        (type_info.sum_center)(samples.get(i)?, samples.dim, &mut agg[c * dim..(c + 1) * dim])?;
    }
    Ok(())
}

// UpdateCenters (ivfkmeans.c:165-174).
fn update_centers(agg: &[f32], centers: &mut VectorArray<'_>, type_info: &IvfflatTypeInfo) -> PgResult<()> {
    let d = centers.dim;
    let dim = d as usize;
    for i in 0..centers.length {
        let x = &agg[i as usize * dim..(i as usize + 1) * dim];
        (type_info.update_center)(centers.get_mut(i)?, d, x);
    }
    Ok(())
}

// ComputeNewCenters (ivfkmeans.c:179-236).
#[allow(clippy::too_many_arguments)]
fn compute_new_centers(
    samples: &VectorArray<'_>,
    agg: &mut [f32],
    new_centers: &mut VectorArray<'_>,
    center_counts: &mut [i32],
    closest_centers: &[i32],
    kmeans_norm: bool,
    collation: Oid,
    type_info: &IvfflatTypeInfo,
) -> PgResult<()> {
    let dimensions = new_centers.dim as usize;
    let num_centers = new_centers.length as usize;

    // Reset sum and count
    for i in 0..num_centers {
        agg[i * dimensions..(i + 1) * dimensions].fill(0.0);
        center_counts[i] = 0;
    }

    // Increment sum of closest center
    sum_centers(samples, agg, closest_centers, type_info)?;

    // Increment count of closest center
    for i in 0..samples.length as usize {
        center_counts[closest_centers[i] as usize] += 1;
    }

    // Divide sum by count
    for i in 0..num_centers {
        let x = &mut agg[i * dimensions..(i + 1) * dimensions];
        if center_counts[i] > 0 {
            // Double avoids overflow, but requires more memory
            for xj in x.iter_mut() {
                if xj.is_infinite() {
                    *xj = if *xj > 0.0 { f32::MAX } else { -f32::MAX };
                }
            }
            for xj in x.iter_mut() {
                *xj /= center_counts[i] as f32;
            }
        } else {
            // TODO Handle empty centers properly
            for xj in x.iter_mut() {
                *xj = random_double() as f32;
            }
        }
    }

    // Set new centers
    update_centers(agg, new_centers, type_info)?;

    // Normalize if needed
    if kmeans_norm {
        norm_centers(type_info, collation, new_centers)?;
    }
    Ok(())
}

// ElkanKmeans (ivfkmeans.c:238-485). Elkan requires the distance to satisfy
// the triangle inequality: L2 distance for L2 (not L2 squared like the index
// scan), angular distance for inner product and cosine.
fn elkan_kmeans(
    procs: &mut KmeansProcs,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    type_info: &IvfflatTypeInfo,
    memory_used: usize,
) -> PgResult<()> {
    let dimensions = centers.dim;
    let num_centers = centers.maxlen;
    let num_samples = samples.length;
    let nc = num_centers as usize;
    let ns = num_samples as usize;

    // Calculate allocation sizes
    let new_centers_size = vector_array_size(num_centers, centers.itemsize)?;
    let agg_size = mcx::mul_size(4, mcx::mul_size(nc, dimensions as usize)?)?;
    let center_counts_size = mcx::mul_size(4, nc)?;
    let closest_centers_size = mcx::mul_size(4, ns)?;
    let lower_bound_size = mcx::mul_size(4, mcx::mul_size(ns, nc)?)?;
    let upper_bound_size = mcx::mul_size(4, ns)?;
    let s_size = mcx::mul_size(4, nc)?;
    let halfcdist_size = mcx::mul_size(4, mcx::mul_size(nc, nc)?)?;
    let newcdist_size = mcx::mul_size(4, nc)?;

    // Calculate total size
    let mut total_size = memory_used;
    for size in [
        new_centers_size,
        agg_size,
        center_counts_size,
        closest_centers_size,
        lower_bound_size,
        upper_bound_size,
        s_size,
        halfcdist_size,
        newcdist_size,
    ] {
        total_size = mcx::add_size(total_size, size)?;
    }

    // Check memory requirements
    ivfflat_check_memory_usage(total_size)?;

    // Ensure indexing does not overflow
    if num_centers > i32::MAX / num_centers {
        return Err(elog_error("Indexing overflow detected. Please report a bug.", "ivfkmeans.c", 294, "ElkanKmeans"));
    }

    // Set support functions
    let collation = procs.collation;
    let kmeans_norm = procs.kmeans_norm.is_some();
    let procinfo = procs.distance.as_mut().expect("k-means distance proc, fetched with samples");

    // Allocate space
    // Use float instead of double to save memory
    let mut agg = vec![0.0f32; nc * dimensions as usize];
    let mut center_counts = vec![0i32; nc];
    let mut closest_centers = vec![0i32; ns];
    let mut lower_bound = vec![0.0f32; ns * nc];
    let mut upper_bound = vec![0.0f32; ns];
    let mut s = vec![0.0f32; nc];
    let mut halfcdist = vec![0.0f32; nc * nc];
    let mut newcdist = vec![0.0f32; nc];

    // Initialize new centers
    let new_centers_cx = mcx::MemoryContext::new("Ivfflat kmeans new centers");
    let mut new_centers = VectorArray::init(new_centers_cx.mcx(), num_centers, dimensions, centers.itemsize)?;
    new_centers.length = num_centers;

    // Pick initial centers
    init_centers(procinfo, collation, samples, centers, &mut lower_bound)?;

    // Assign each x to its closest initial center c(x) = argmin d(x,c)
    for j in 0..ns {
        let mut min_distance = f32::MAX;
        let mut closest_center = 0i32;

        // Find closest center
        for k in 0..nc {
            // TODO Use Lemma 1 in k-means++ initialization
            let d = lower_bound[j * nc + k];
            if d < min_distance {
                min_distance = d;
                closest_center = k as i32;
            }
        }

        upper_bound[j] = min_distance;
        closest_centers[j] = closest_center;
    }

    // Give 500 iterations to converge
    for iteration in 0..500 {
        let mut changes = 0;

        // Can take a while, so ensure we can interrupt
        postgres_seams::check_for_interrupts::call()?;

        // Step 1: For all centers, compute distance
        for j in 0..nc {
            let vec = centers.datum(j as i32)?;
            for k in j + 1..nc {
                let d = (0.5 * distance(procinfo, collation, vec, centers.datum(k as i32)?)?) as f32;
                halfcdist[j * nc + k] = d;
                halfcdist[k * nc + j] = d;
            }
        }

        // For all centers c, compute s(c)
        for j in 0..nc {
            let mut min_distance = f32::MAX;
            for k in 0..nc {
                if j == k {
                    continue;
                }
                let d = halfcdist[j * nc + k];
                if d < min_distance {
                    min_distance = d;
                }
            }
            s[j] = min_distance;
        }

        let rjreset = iteration != 0;

        for j in 0..ns {
            // Step 2: Identify all points x such that u(x) <= s(c(x))
            if upper_bound[j] <= s[closest_centers[j] as usize] {
                continue;
            }

            let mut rj = rjreset;

            for k in 0..nc {
                // Step 3: For all remaining points x and centers c
                if k as i32 == closest_centers[j] {
                    continue;
                }
                if upper_bound[j] <= lower_bound[j * nc + k] {
                    continue;
                }
                if upper_bound[j] <= halfcdist[closest_centers[j] as usize * nc + k] {
                    continue;
                }

                let vec = samples.datum(j as i32)?;

                // Step 3a
                let dxcx: f32 = if rj {
                    let c = closest_centers[j];
                    let dxcx = distance(procinfo, collation, vec, centers.datum(c)?)? as f32;
                    // d(x,c(x)) computed, which is a form of d(x,c)
                    lower_bound[j * nc + c as usize] = dxcx;
                    upper_bound[j] = dxcx;
                    rj = false;
                    dxcx
                } else {
                    upper_bound[j]
                };

                // Step 3b
                if dxcx > lower_bound[j * nc + k] || dxcx > halfcdist[closest_centers[j] as usize * nc + k] {
                    let dxc = distance(procinfo, collation, vec, centers.datum(k as i32)?)? as f32;

                    // d(x,c) calculated
                    lower_bound[j * nc + k] = dxc;

                    if dxc < dxcx {
                        closest_centers[j] = k as i32;
                        // c(x) changed
                        upper_bound[j] = dxc;
                        changes += 1;
                    }
                }
            }
        }

        // Step 4: For each center c, let m(c) be mean of all points assigned
        compute_new_centers(
            samples,
            &mut agg,
            &mut new_centers,
            &mut center_counts,
            &closest_centers,
            kmeans_norm,
            collation,
            type_info,
        )?;

        // Step 5
        for j in 0..nc {
            newcdist[j] =
                distance(procinfo, collation, centers.datum(j as i32)?, new_centers.datum(j as i32)?)? as f32;
        }

        for j in 0..ns {
            for k in 0..nc {
                let mut d = lower_bound[j * nc + k] - newcdist[k];
                if d < 0.0 {
                    d = 0.0;
                }
                lower_bound[j * nc + k] = d;
            }
        }

        // Step 6
        // We reset r(x) before Step 3 in the next iteration
        for j in 0..ns {
            upper_bound[j] += newcdist[closest_centers[j] as usize];
        }

        // Step 7
        for j in 0..num_centers {
            centers.set(j, new_centers.get(j)?)?;
        }

        if changes == 0 && iteration != 0 {
            break;
        }
    }
    Ok(())
}

// CheckElements (ivfkmeans.c:490-512): ensure no NaN or infinite values.
fn check_elements(centers: &VectorArray<'_>, type_info: &IvfflatTypeInfo) -> PgResult<()> {
    let mut scratch = vec![0.0f32; centers.dim as usize];
    for i in 0..centers.length {
        scratch.fill(0.0);
        // /fp:fast may not propagate NaN with MSVC, but that's alright
        (type_info.sum_center)(centers.get(i)?, centers.dim, &mut scratch)?;
        for x in scratch.iter() {
            if x.is_nan() {
                return Err(elog_error("NaN detected. Please report a bug.", "ivfkmeans.c", 506, "CheckElements"));
            }
            if x.is_infinite() {
                return Err(elog_error(
                    "Infinite value detected. Please report a bug.",
                    "ivfkmeans.c",
                    509,
                    "CheckElements",
                ));
            }
        }
    }
    Ok(())
}

// CheckNorms (ivfkmeans.c:517-534): ensure no zero vectors for cosine
// distance. It checks NORM_PROC, not KMEANS_NORM_PROC.
fn check_norms(centers: &VectorArray<'_>, procs: &mut KmeansProcs) -> PgResult<()> {
    let collation = procs.collation;
    let Some(normprocinfo) = procs.norm.as_mut() else {
        return Ok(());
    };
    for i in 0..centers.length {
        let norm = types_fmgr::function_call1_coll(normprocinfo, collation, centers.datum(i)?)?.as_f64();
        if norm == 0.0 {
            return Err(elog_error("Zero norm detected. Please report a bug.", "ivfkmeans.c", 532, "CheckNorms"));
        }
    }
    Ok(())
}

// CheckCenters (ivfkmeans.c:539-547): detect issues with centers.
fn check_centers(centers: &VectorArray<'_>, type_info: &IvfflatTypeInfo, procs: &mut KmeansProcs) -> PgResult<()> {
    if centers.length != centers.maxlen {
        return Err(elog_error("Not enough centers. Please report a bug.", "ivfkmeans.c", 543, "CheckCenters"));
    }
    check_elements(centers, type_info)?;
    check_norms(centers, procs)
}

// IvfflatKmeans (ivfkmeans.c:549-570): spherical k-means for inner product
// and cosine.
pub fn ivfflat_kmeans(
    index: &Relation<'_>,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    type_info: &IvfflatTypeInfo,
    memory_used: usize,
) -> PgResult<()> {
    let mut procs = KmeansProcs {
        collation: index_collation(index),
        distance: if samples.length == 0 {
            None
        } else {
            Some(index_getprocinfo(index, IVFFLAT_KMEANS_DISTANCE_PROC)?)
        },
        kmeans_norm: ivfflat_optional_proc_info(index, IVFFLAT_KMEANS_NORM_PROC)?,
        norm: ivfflat_optional_proc_info(index, IVFFLAT_NORM_PROC)?,
    };
    kmeans(&mut procs, samples, centers, type_info, memory_used)
}

// IvfflatKmeans' body over resolved support procs.
pub fn kmeans(
    procs: &mut KmeansProcs,
    samples: &VectorArray<'_>,
    centers: &mut VectorArray<'_>,
    type_info: &IvfflatTypeInfo,
    memory_used: usize,
) -> PgResult<()> {
    if samples.length == 0 {
        random_centers(procs, centers, type_info)?;
    } else {
        elkan_kmeans(procs, samples, centers, type_info, memory_used)?;
    }
    check_centers(centers, type_info, procs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgvector::vec::{VecBuilder, VecView};
    use pgvector_ivfflat::utils::VECTOR_TYPE_INFO;

    // CHECK_FOR_INTERRUPTS has no owner in a unit-test binary: install a
    // no-op once.
    fn install_cfi_stub() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| postgres_seams::check_for_interrupts::set(|| Ok(())));
    }

    fn l2_procs() -> KmeansProcs {
        install_cfi_stub();
        KmeansProcs {
            collation: 0,
            distance: Some(types_fmgr::FmgrInfo::new(pgvector::funcs::fc_l2_distance, 1, 2, true, false)),
            kmeans_norm: None,
            norm: None,
        }
    }

    fn sample_array<'m>(m: mcx::Mcx<'m>, maxlen: i32, points: &[[f32; 2]]) -> VectorArray<'m> {
        let mut arr = VectorArray::init(m, maxlen, 2, 16).unwrap();
        for (i, p) in points.iter().enumerate() {
            let mut b = VecBuilder::new(m, 2).unwrap();
            b.set(0, p[0]);
            b.set(1, p[1]);
            arr.set(i as i32, &b.image()).unwrap();
            arr.length += 1;
        }
        arr
    }

    fn center(arr: &VectorArray<'_>, i: i32) -> [f32; 2] {
        let v = VecView::from_payload(&arr.get(i).unwrap()[4..16]).unwrap();
        [v.x(0), v.x(1)]
    }

    // Elkan k-means (ivfkmeans.c:246-485) on two well-separated groups of
    // four points ends on each group's exact mean (the bytecmp cases' shape).
    #[test]
    fn ivf_kmeans_converges_to_group_means() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let pts = [
            [0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0],
            [256.0, 0.0], [257.0, 0.0], [256.0, 1.0], [257.0, 1.0],
        ];
        let samples = sample_array(m, 8, &pts);
        let mut centers = VectorArray::init(m, 2, 2, 16).unwrap();
        pg_prng::global_prng(|p| p.seed(42));
        kmeans(&mut l2_procs(), &samples, &mut centers, &VECTOR_TYPE_INFO, 0).unwrap();
        let mut got = [center(&centers, 0), center(&centers, 1)];
        got.sort_by(|a, b| a[0].total_cmp(&b[0]));
        assert_eq!(got, [[0.5, 0.5], [256.5, 0.5]]);
    }

    // Review Focus 1: three distinct values for five centers. k-means++ runs
    // out of weight and picks sample 0 again (ivfkmeans.c:78-84), empty
    // clusters get random centers (:224-227), and every center is filled and
    // finite (CheckCenters).
    #[test]
    fn ivf_kmeans_fills_every_center_from_duplicates() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let pts = [[1.0, 1.0], [1.0, 1.0], [1.0, 1.0], [5.0, 5.0], [5.0, 5.0], [9.0, 9.0]];
        let samples = sample_array(m, 6, &pts);
        let mut centers = VectorArray::init(m, 5, 2, 16).unwrap();
        pg_prng::global_prng(|p| p.seed(7));
        kmeans(&mut l2_procs(), &samples, &mut centers, &VECTOR_TYPE_INFO, 0).unwrap();
        assert_eq!(centers.length, 5);
        let got: Vec<[f32; 2]> = (0..5).map(|i| center(&centers, i)).collect();
        for p in [[1.0, 1.0], [5.0, 5.0], [9.0, 9.0]] {
            assert!(got.contains(&p), "{got:?}");
        }
    }

    // RandomCenters (ivfkmeans.c:110-133): with no samples (an empty table, or
    // an unlogged index's init fork) every center comes from RandomDouble, in
    // center order, then dimension order.
    #[test]
    fn ivf_kmeans_random_centers_without_samples() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let samples = VectorArray::init(m, 1, 2, 16).unwrap();
        let mut centers = VectorArray::init(m, 3, 2, 16).unwrap();
        let mut procs = KmeansProcs { collation: 0, distance: None, kmeans_norm: None, norm: None };
        pg_prng::global_prng(|p| p.seed(42));
        kmeans(&mut procs, &samples, &mut centers, &VECTOR_TYPE_INFO, 0).unwrap();
        let mut want = pg_prng::PgPrng::seeded(42);
        for i in 0..3 {
            assert_eq!(center(&centers, i), [want.next_f64() as f32, want.next_f64() as f32]);
        }
    }

    // CheckCenters and CheckElements (ivfkmeans.c:490-547).
    #[test]
    fn ivf_check_centers_rejects_short_arrays_and_nan() {
        let ctx = mcx::MemoryContext::new("ivf-test");
        let m = ctx.mcx();
        let mut procs = KmeansProcs { collation: 0, distance: None, kmeans_norm: None, norm: None };
        let short = sample_array(m, 2, &[[1.0, 1.0]]);
        let e = check_centers(&short, &VECTOR_TYPE_INFO, &mut procs).unwrap_err();
        assert_eq!(e.message(), "Not enough centers. Please report a bug.");
        let nan = sample_array(m, 2, &[[1.0, 1.0], [f32::NAN, 0.0]]);
        let e = check_centers(&nan, &VECTOR_TYPE_INFO, &mut procs).unwrap_err();
        assert_eq!(e.message(), "NaN detected. Please report a bug.");
        let inf = sample_array(m, 1, &[[f32::INFINITY, 0.0]]);
        let e = check_centers(&inf, &VECTOR_TYPE_INFO, &mut procs).unwrap_err();
        assert_eq!(e.message(), "Infinite value detected. Please report a bug.");
    }
}
