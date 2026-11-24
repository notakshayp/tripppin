//! How to run a model on this machine without running out of memory: where the layers go
//! (CPU, an integrated GPU sharing system memory, or a graphics card), how long a context
//! fits next to the desktop, and how the KV cache is stored.
//!
//! Everything here is a pure function of the hardware and the model's shape, so each
//! decision is tested with made-up machines. `trip model serve` turns the result into
//! llama.cpp server arguments.

use crate::gguf::ModelShape;
use crate::{Hardware, Vendor};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// The shortest context the agent can work in: system prompt, tools, notebook and a few steps.
pub const MIN_CONTEXT: u32 = 8192;
/// Compute buffers (attention scratch, logits for a 512-token batch) and the allocator's slack.
const COMPUTE: u64 = 768 * MIB;
/// Room left on an integrated GPU's share of memory for the desktop's own graphics.
const IGPU_MARGIN_MIB: u64 = 2048;
/// Tokens processed per batch; the sliding-window cache holds this many on top of its window.
const UBATCH: u64 = 512;

/// The GPU the model can use, as far as memory goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Accel {
    /// No usable GPU (or a virtual one): run on the CPU.
    None,
    /// An integrated GPU (AMD APU, Intel): it shares system memory, and the kernel lets it map
    /// at most `pool_bytes` of it (on AMD, the BIOS carve-out plus the GTT, about half the RAM).
    Integrated { pool_bytes: u64 },
    /// A graphics card with its own memory.
    Dedicated { vram_bytes: u64 },
}

impl Accel {
    /// What the hardware offers. `igpu_pool` is what the integrated GPU may map, when the
    /// driver says (see [`crate::igpu_pool_bytes`]); otherwise half the memory is assumed.
    pub fn from_hardware(hw: &Hardware, igpu_pool: Option<u64>) -> Accel {
        if let Some(vram) = hw.gpus.iter().filter_map(|g| g.vram_bytes).max() {
            return Accel::Dedicated { vram_bytes: vram };
        }
        if hw.gpus.iter().any(|g| matches!(g.vendor, Vendor::Amd | Vendor::Intel)) {
            let pool = igpu_pool.unwrap_or(hw.memory_bytes / 2).min(hw.memory_bytes);
            return Accel::Integrated { pool_bytes: pool };
        }
        Accel::None
    }
}

/// Where the model's layers go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Everything on the CPU; GPUs are left alone.
    Cpu,
    /// On the GPU, with llama.cpp's `--fit` choosing how many layers fit, keeping
    /// `margin_mib` of the GPU's memory free.
    Gpu { margin_mib: u64 },
}

/// How the KV cache is stored. q8_0 halves it next to f16 at no measurable cost in quality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheType {
    F16,
    Q8_0,
}

impl CacheType {
    pub fn name(self) -> &'static str {
        match self {
            CacheType::F16 => "f16",
            CacheType::Q8_0 => "q8_0",
        }
    }

    /// Bytes per stored value, times 32 (q8_0 stores 32 values in 34 bytes).
    fn bytes_x32(self) -> u64 {
        match self {
            CacheType::F16 => 64,
            CacheType::Q8_0 => 34,
        }
    }
}

/// How to serve a model on a machine.
#[derive(Clone, Debug, PartialEq)]
pub struct ServePlan {
    pub placement: Placement,
    /// Context length in tokens.
    pub context: u32,
    pub cache: CacheType,
    /// Upper bound for llama.cpp's prompt cache in system memory (other conversations kept
    /// for later, so switching between the orb and the terminal doesn't start over).
    pub cache_ram_mib: u64,
    /// Snapshots per conversation of the state that can't be rewound (recurrent layers,
    /// sliding windows), so a changed ending doesn't mean reading everything again.
    pub checkpoints: u32,
    /// Estimated memory the model takes when its context is full, in bytes.
    pub estimate_bytes: u64,
    /// One line for the log: why these settings.
    pub why: String,
}

/// Memory the desktop keeps for itself: a quarter of the RAM, at least 4 GiB.
pub fn desktop_reserve(memory_bytes: u64) -> u64 {
    (memory_bytes / 4).max(4 * GIB)
}

/// KV cache bytes for `tokens` of context, including the fixed sliding-window part.
pub fn kv_bytes(shape: &ModelShape, cache: CacheType, tokens: u32) -> u64 {
    let full = shape.kv_values_full * u64::from(tokens);
    let window = (u64::from(shape.swa_window) + UBATCH).min(u64::from(tokens));
    let swa = shape.kv_values_swa * window;
    (full + swa) * cache.bytes_x32() / 32
}

/// Picks placement, context length and cache settings for `shape` (a `model_bytes` file) so
/// that the model, its full context and llama.cpp's caches fit next to the desktop. The
/// context is at most `wanted` (the tier's) and the model's trained length, and at least
/// [`MIN_CONTEXT`].
pub fn plan(memory_bytes: u64, accel: Accel, shape: &ModelShape, model_bytes: u64, wanted: u32) -> ServePlan {
    let cache = CacheType::Q8_0;
    let big = memory_bytes >= 24 * GIB;
    let cache_ram_mib = if big { 2048 } else { 1024 };
    let checkpoints = if big { 16 } else { 8 };
    // Each checkpoint holds the recurrent state and the sliding-window cache.
    let checkpoint_bytes =
        shape.recurrent_bytes + kv_bytes(&ModelShape { kv_values_full: 0, ..shape.clone() }, cache, u32::MAX);
    let host_extra = cache_ram_mib * MIB
        + if shape.recurrent_layers > 0 || shape.kv_values_swa > 0 {
            u64::from(checkpoints) * checkpoint_bytes
        } else {
            0
        };
    let ram_budget = memory_bytes.saturating_sub(desktop_reserve(memory_bytes));
    let wanted = wanted.min(shape.train_context.max(MIN_CONTEXT)).max(MIN_CONTEXT);
    let fixed = model_bytes + COMPUTE + shape.recurrent_bytes;
    let cost = |ctx: u32| fixed + kv_bytes(shape, cache, ctx);
    // The longest context (a multiple of 1024, at most `wanted`) whose model, cache and
    // buffers fit in `pool` bytes; `None` if not even the minimum does.
    let longest = |pool: u64| -> Option<u32> {
        if cost(MIN_CONTEXT) > pool {
            return None;
        }
        let per_1k = kv_bytes(shape, cache, MIN_CONTEXT + 1024) - kv_bytes(shape, cache, MIN_CONTEXT);
        let spare = pool - cost(MIN_CONTEXT);
        let more = spare.checked_div(per_1k).map_or(u64::from(wanted), |k| k * 1024);
        Some((u64::from(MIN_CONTEXT) + more).min(u64::from(wanted)) as u32)
    };
    let ram_pool = ram_budget.saturating_sub(host_extra);
    // When the weights alone fill the budget (a big MoE model on 32 GB), the kernel pages them
    // from disk as needed; the context then gets at most an eighth of the budget.
    let squeezed = || {
        let allowance = ram_budget / 8;
        let per_1k = kv_bytes(shape, cache, MIN_CONTEXT + 1024) - kv_bytes(shape, cache, MIN_CONTEXT);
        let spare = allowance.saturating_sub(kv_bytes(shape, cache, MIN_CONTEXT));
        let more = spare.checked_div(per_1k).map_or(u64::from(wanted), |k| k * 1024);
        (u64::from(MIN_CONTEXT) + more).min(u64::from(wanted)) as u32
    };
    let in_ram = || longest(ram_pool).unwrap_or_else(squeezed);
    let gib = |b: u64| b as f64 / GIB as f64;

    let (placement, context, why) = match accel {
        Accel::Dedicated { vram_bytes } => {
            // llama.cpp's --fit puts as many layers on the card as fit; the rest run from RAM.
            let ctx = longest(vram_bytes.saturating_sub(GIB) + ram_pool).unwrap_or_else(squeezed);
            (Placement::Gpu { margin_mib: 1024 }, ctx, format!("graphics card with {:.1} GiB", gib(vram_bytes)))
        }
        Accel::Integrated { pool_bytes } => {
            let gpu_pool = pool_bytes.saturating_sub(IGPU_MARGIN_MIB * MIB).min(ram_pool);
            match longest(gpu_pool) {
                Some(ctx) => (
                    Placement::Gpu { margin_mib: IGPU_MARGIN_MIB },
                    ctx,
                    format!("integrated GPU, {:.1} GiB of shared memory usable", gib(pool_bytes)),
                ),
                None => (
                    Placement::Cpu,
                    in_ram(),
                    format!(
                        "integrated GPU can map only {:.1} GiB, too little for this model; using the CPU",
                        gib(pool_bytes)
                    ),
                ),
            }
        }
        Accel::None => (Placement::Cpu, in_ram(), "no GPU; using the CPU".to_string()),
    };
    ServePlan {
        placement,
        context,
        cache,
        cache_ram_mib,
        checkpoints,
        estimate_bytes: cost(context),
        why: format!(
            "{why}; {:.1} GiB of {:.1} GiB memory for the model, {context} tokens of context",
            gib(ram_budget),
            gib(memory_bytes)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gguf::{parse_shape, tests as g};
    use crate::{Cpu, Gpu};

    /// What the kernel reports for a "16 GB" and a "32 GB" laptop.
    const RAM_16: u64 = 15_300 * MIB;
    const RAM_32: u64 = 30_600 * MIB;
    const QWEN_4B: u64 = 2_740_937_888;
    const QWEN_35B_A3B: u64 = 22_016_023_168;
    const GRANITE_8B: u64 = 5_347_915_136;

    fn qwen() -> ModelShape {
        parse_shape(&mut g::qwen35_4b().as_slice()).unwrap()
    }

    fn granite() -> ModelShape {
        parse_shape(&mut g::granite_8b().as_slice()).unwrap()
    }

    fn laptop(memory: u64, gpus: Vec<Gpu>) -> Hardware {
        Hardware {
            memory_bytes: memory,
            cpu: Cpu { model: "AMD Ryzen 7 7840U".into(), threads: 16, flags: vec![] },
            gpus,
        }
    }

    fn amd_igpu() -> Gpu {
        Gpu { vendor: Vendor::Amd, name: None, vram_bytes: None }
    }

    #[test]
    fn reads_the_gpu_from_the_hardware() {
        let hw = laptop(RAM_16, vec![amd_igpu()]);
        assert_eq!(Accel::from_hardware(&hw, Some(8 * GIB)), Accel::Integrated { pool_bytes: 8 * GIB });
        assert_eq!(Accel::from_hardware(&hw, None), Accel::Integrated { pool_bytes: RAM_16 / 2 });
        let card = Gpu { vendor: Vendor::Nvidia, name: None, vram_bytes: Some(8 * GIB) };
        let hw = laptop(RAM_16, vec![amd_igpu(), card]);
        assert_eq!(Accel::from_hardware(&hw, None), Accel::Dedicated { vram_bytes: 8 * GIB });
        let vm = laptop(RAM_16, vec![Gpu { vendor: Vendor::Other, name: None, vram_bytes: None }]);
        assert_eq!(Accel::from_hardware(&vm, None), Accel::None);
    }

    #[test]
    fn a_16_gb_amd_laptop_runs_the_small_model_on_its_igpu_with_the_full_context() {
        let p = plan(RAM_16, Accel::Integrated { pool_bytes: RAM_16 / 2 + 512 * MIB }, &qwen(), QWEN_4B, 32768);
        assert_eq!(p.placement, Placement::Gpu { margin_mib: 2048 });
        assert_eq!(p.context, 32768);
        assert_eq!(p.cache, CacheType::Q8_0);
        assert_eq!((p.cache_ram_mib, p.checkpoints), (1024, 8));
        // The KV cache for 32k tokens is about half a gigabyte at q8_0.
        assert_eq!(kv_bytes(&qwen(), CacheType::Q8_0, 32768), 8 * 4 * 512 * 32768 * 34 / 32);
        assert!(p.estimate_bytes < 5 * GIB, "{}", p.estimate_bytes);
    }

    #[test]
    fn a_big_kv_cache_shrinks_the_context_to_fit_the_igpu() {
        // Granite 8B keeps 160 KiB per token at f16 (85 at q8_0); 32k tokens won't fit in 9 GiB.
        let p = plan(RAM_16, Accel::Integrated { pool_bytes: 9 * GIB }, &granite(), GRANITE_8B, 32768);
        assert_eq!(p.placement, Placement::Gpu { margin_mib: 2048 });
        assert!(p.context > MIN_CONTEXT && p.context < 32768, "{}", p.context);
        assert_eq!(p.context % 1024, 0);
        assert!(p.estimate_bytes <= 9 * GIB - 2048 * MIB);
        // With the default GTT (half the RAM) not even 8k fits next to its 5 GB of weights.
        let p = plan(RAM_16, Accel::Integrated { pool_bytes: RAM_16 / 2 }, &granite(), GRANITE_8B, 32768);
        assert_eq!(p.placement, Placement::Cpu);
        assert!(p.context >= 16384, "{}", p.context);
    }

    #[test]
    fn a_model_too_big_for_the_igpu_runs_on_the_cpu() {
        // 35B-A3B is 20.5 GiB; the iGPU of a 32 GB laptop may map about 15 GiB.
        let shape = ModelShape { kv_values_full: 10 * 2 * 512, recurrent_layers: 30, ..qwen() };
        let p = plan(RAM_32, Accel::Integrated { pool_bytes: RAM_32 / 2 }, &shape, QWEN_35B_A3B, 16384);
        assert_eq!(p.placement, Placement::Cpu);
        assert_eq!(p.context, 16384);
        assert_eq!((p.cache_ram_mib, p.checkpoints), (2048, 16));
        assert!(p.why.contains("using the CPU"));
    }

    #[test]
    fn without_a_gpu_the_context_fits_next_to_the_desktop() {
        let p = plan(RAM_16, Accel::None, &granite(), GRANITE_8B, 32768);
        assert_eq!(p.placement, Placement::Cpu);
        // 15 GiB less 4 GiB for the desktop, 1 GiB of prompt cache, the model and buffers.
        let budget = RAM_16 - desktop_reserve(RAM_16) - 1024 * MIB;
        assert!(p.estimate_bytes <= budget, "{} > {budget}", p.estimate_bytes);
        assert!(p.context >= 16384);
    }

    #[test]
    fn never_below_the_minimum_or_above_what_was_asked() {
        let tiny = plan(8 * GIB, Accel::None, &granite(), GRANITE_8B, 32768);
        assert_eq!(tiny.context, MIN_CONTEXT);
        let short = ModelShape { train_context: 4096, ..qwen() };
        assert_eq!(plan(RAM_32, Accel::None, &short, QWEN_4B, 32768).context, MIN_CONTEXT);
        assert_eq!(plan(RAM_32, Accel::None, &qwen(), QWEN_4B, 16384).context, 16384);
    }

    #[test]
    fn a_graphics_card_lets_llama_cpp_fit_the_layers() {
        let p = plan(RAM_16, Accel::Dedicated { vram_bytes: 8 * GIB }, &qwen(), QWEN_4B, 16384);
        assert_eq!(p.placement, Placement::Gpu { margin_mib: 1024 });
        assert_eq!(p.context, 16384);
    }

    #[test]
    fn sliding_windows_cost_a_fixed_amount() {
        let s = ModelShape { kv_values_full: 1000, kv_values_swa: 100, swa_window: 512, ..Default::default() };
        assert_eq!(kv_bytes(&s, CacheType::F16, 100), (1000 * 100 + 100 * 100) * 2);
        assert_eq!(kv_bytes(&s, CacheType::F16, 50_000), (1000 * 50_000 + 100 * 1024) * 2);
    }
}
