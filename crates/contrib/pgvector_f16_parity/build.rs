//! Compiles pgvector 0.8.7's own f16 code for the parity tests in
//! src/lib.rs: the software routines cut verbatim from the vendored
//! halfutils.h (the path C takes when neither F16C_SUPPORT nor FLT16_SUPPORT
//! is defined), next to the compiler's `_Float16` conversions and
//! arithmetic (the FLT16_SUPPORT path the macOS arm64 reference build
//! takes). Skipped on wasm32 (no C toolchain there), which compiles the
//! tests out (`cfg(pgv_c_half)`).
use std::{env, fs, path::PathBuf};

const HALFUTILS: &str = "../../pgvector-0.8.7-reference/src/halfutils.h";

/// One `static inline` function of halfutils.h, keeping only its software
/// `#else` branch.
fn software_branch(src: &str, signature: &str) -> String {
    let start = src.find(signature).unwrap_or_else(|| panic!("{HALFUTILS}: no {signature:?}"));
    let end = start + src[start..].find("\n}\n").expect("function end") + "\n}\n".len();
    let f = &src[start..end];
    let cond = f.find("#if defined(F16C_SUPPORT)").expect("#if defined(F16C_SUPPORT)");
    let soft = cond + f[cond..].find("#else\n").expect("#else") + "#else\n".len();
    let endif = f.rfind("#endif\n").expect("#endif");
    format!("{}{}{}", &f[..cond], &f[soft..endif], &f[endif + "#endif\n".len()..])
}

const SHIMS: &str = r#"
uint32_t pgv_sw_half_to_float4(uint16_t h) { union { float f; uint32_t i; } u; u.f = HalfToFloat4(h); return u.i; }
uint16_t pgv_sw_float4_to_half(uint32_t bits) { union { float f; uint32_t i; } u; u.i = bits; return Float4ToHalfUnchecked(u.f); }
#ifdef __FLT16_MAX__
typedef union { _Float16 h; uint16_t i; } pgv_hu;
int pgv_has_float16(void) { return 1; }
uint32_t pgv_hw_half_to_float4(uint16_t h) { pgv_hu x; union { float f; uint32_t i; } u; x.i = h; u.f = (float) x.h; return u.i; }
uint16_t pgv_hw_float4_to_half(uint32_t bits) { pgv_hu r; union { float f; uint32_t i; } u; u.i = bits; r.h = (_Float16) u.f; return r.i; }
uint16_t pgv_hw_add(uint16_t a, uint16_t b) { pgv_hu x, y, r; x.i = a; y.i = b; r.h = x.h + y.h; return r.i; }
uint16_t pgv_hw_sub(uint16_t a, uint16_t b) { pgv_hu x, y, r; x.i = a; y.i = b; r.h = x.h - y.h; return r.i; }
uint16_t pgv_hw_mul(uint16_t a, uint16_t b) { pgv_hu x, y, r; x.i = a; y.i = b; r.h = x.h * y.h; return r.i; }
#else
int pgv_has_float16(void) { return 0; }
uint32_t pgv_hw_half_to_float4(uint16_t h) { (void) h; return 0; }
uint16_t pgv_hw_float4_to_half(uint32_t bits) { (void) bits; return 0; }
uint16_t pgv_hw_add(uint16_t a, uint16_t b) { (void) a; (void) b; return 0; }
uint16_t pgv_hw_sub(uint16_t a, uint16_t b) { (void) a; (void) b; return 0; }
uint16_t pgv_hw_mul(uint16_t a, uint16_t b) { (void) a; (void) b; return 0; }
#endif
"#;

fn main() {
    println!("cargo:rerun-if-changed={HALFUTILS}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(pgv_c_half)");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }
    let src = fs::read_to_string(HALFUTILS).expect("read halfutils.h");
    let c = format!(
        "#include <math.h>\n#include <stdint.h>\n\
         typedef uint16_t uint16;\ntypedef uint32_t uint32;\ntypedef uint16 half;\n\
         #define unlikely(x) (x)\n\n{}\n{}\n{}",
        software_branch(&src, "static inline float\nHalfToFloat4(half num)"),
        software_branch(&src, "static inline half\nFloat4ToHalfUnchecked(float num)"),
        SHIMS
    );
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("halfutils_parity.c");
    fs::write(&out, c).expect("write halfutils_parity.c");
    cc::Build::new().file(&out).warnings(false).compile("pgv_halfutils_parity");
    println!("cargo:rustc-cfg=pgv_c_half");
}
