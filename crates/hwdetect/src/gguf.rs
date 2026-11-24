//! Reads the shape of a model from its GGUF header: how many layers keep a KV cache, how big
//! each token's entry is, and which layers use a sliding window or a recurrent state instead.
//! That's what decides how much memory a context of a given length takes.
//!
//! Only the metadata at the start of the file is read (a few megabytes, mostly the
//! tokenizer, which is skipped); the weights are never touched.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::{self, BufReader, Read};
use std::path::Path;

/// What a model's context costs, per token and per conversation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelShape {
    pub arch: String,
    pub layers: u32,
    /// The longest context the model was trained for.
    pub train_context: u32,
    /// K and V values per token over the layers that attend to the whole context.
    pub kv_values_full: u64,
    /// K and V values per token over the sliding-window layers.
    pub kv_values_swa: u64,
    /// How many tokens the sliding-window layers keep.
    pub swa_window: u32,
    /// Layers with a recurrent state instead of a KV cache (Qwen3.5's linear attention, Mamba).
    pub recurrent_layers: u32,
    /// Bytes of recurrent state per conversation, over all those layers.
    pub recurrent_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Ints(Vec<i64>),
    Bools(Vec<bool>),
    /// An array we don't need (the tokenizer's), skipped.
    Other,
}

impl Value {
    fn int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
}

const MAX_KEYS: u64 = 1 << 16;
const MAX_STRING: u64 = 1 << 26;
/// Arrays this short are kept (per-layer settings); longer ones are skipped.
const MAX_KEPT_ARRAY: u64 = 4096;

/// Reads the shape of the model at `path`.
pub fn read_shape(path: &Path) -> Result<ModelShape> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    parse_shape(&mut BufReader::with_capacity(1 << 20, file)).with_context(|| format!("reading {}", path.display()))
}

/// Reads the shape from a GGUF stream positioned at its start.
pub fn parse_shape(r: &mut impl Read) -> Result<ModelShape> {
    let meta = read_metadata(r)?;
    shape_from(&meta)
}

fn read_metadata(r: &mut impl Read) -> Result<HashMap<String, Value>> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if &magic != b"GGUF" {
        bail!("not a GGUF file");
    }
    let version = u32_(r)?;
    if !(2..=3).contains(&version) {
        bail!("unsupported GGUF version {version}");
    }
    let _tensors = u64_(r)?;
    let keys = u64_(r)?;
    if keys > MAX_KEYS {
        bail!("implausible metadata count {keys}");
    }
    let mut meta = HashMap::new();
    for _ in 0..keys {
        let key = string(r)?;
        let ty = u32_(r)?;
        let value = value(r, ty)?;
        meta.insert(key, value);
    }
    Ok(meta)
}

fn value(r: &mut impl Read, ty: u32) -> Result<Value> {
    Ok(match ty {
        0 => Value::Int(i64::from(bytes::<1>(r)?[0])),
        1 => Value::Int(i64::from(bytes::<1>(r)?[0] as i8)),
        2 => Value::Int(i64::from(u16::from_le_bytes(bytes(r)?))),
        3 => Value::Int(i64::from(i16::from_le_bytes(bytes(r)?))),
        4 => Value::Int(i64::from(u32_(r)?)),
        5 => Value::Int(i64::from(i32::from_le_bytes(bytes(r)?))),
        6 => Value::Float(f64::from(f32::from_le_bytes(bytes(r)?))),
        7 => Value::Bool(bytes::<1>(r)?[0] != 0),
        8 => Value::Str(string(r)?),
        9 => {
            let elem = u32_(r)?;
            let n = u64_(r)?;
            if n > MAX_KEPT_ARRAY || !matches!(elem, 0..=5 | 7 | 10 | 11) {
                skip_array(r, elem, n)?;
                return Ok(Value::Other);
            }
            let items: Vec<Value> = (0..n).map(|_| value(r, elem)).collect::<Result<_>>()?;
            if elem == 7 {
                Value::Bools(items.iter().map(|v| matches!(v, Value::Bool(true))).collect())
            } else {
                Value::Ints(items.iter().filter_map(Value::int).collect())
            }
        }
        10 => Value::Int(u64_(r)? as i64),
        11 => Value::Int(i64::from_le_bytes(bytes(r)?)),
        12 => Value::Float(f64::from_le_bytes(bytes(r)?)),
        other => bail!("unknown GGUF value type {other}"),
    })
}

fn skip_array(r: &mut impl Read, elem: u32, n: u64) -> Result<()> {
    let size = match elem {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4..=6 => 4,
        10..=12 => 8,
        8 => {
            for _ in 0..n {
                let len = u64_(r)?;
                skip(r, len)?;
            }
            return Ok(());
        }
        9 => {
            for _ in 0..n {
                let inner = u32_(r)?;
                let m = u64_(r)?;
                skip_array(r, inner, m)?;
            }
            return Ok(());
        }
        other => bail!("unknown GGUF array type {other}"),
    };
    skip(r, n.checked_mul(size).context("array too large")?)
}

fn skip(r: &mut impl Read, n: u64) -> Result<()> {
    let copied = io::copy(&mut r.by_ref().take(n), &mut io::sink())?;
    if copied != n {
        bail!("the file ends inside its metadata");
    }
    Ok(())
}

fn bytes<const N: usize>(r: &mut impl Read) -> Result<[u8; N]> {
    let mut b = [0u8; N];
    r.read_exact(&mut b)?;
    Ok(b)
}

fn u32_(r: &mut impl Read) -> Result<u32> {
    Ok(u32::from_le_bytes(bytes(r)?))
}

fn u64_(r: &mut impl Read) -> Result<u64> {
    Ok(u64::from_le_bytes(bytes(r)?))
}

fn string(r: &mut impl Read) -> Result<String> {
    let len = u64_(r)?;
    if len > MAX_STRING {
        bail!("implausible string length {len}");
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn shape_from(meta: &HashMap<String, Value>) -> Result<ModelShape> {
    let arch = match meta.get("general.architecture") {
        Some(Value::Str(s)) => s.clone(),
        _ => bail!("the model doesn't say its architecture"),
    };
    let get = |k: &str| meta.get(&format!("{arch}.{k}"));
    let int = |k: &str| get(k).and_then(Value::int).filter(|&n| n >= 0).map(|n| n as u64);
    let layers = int("block_count").context("the model doesn't say how many layers it has")? as u32;
    let heads = int("attention.head_count").unwrap_or(1).max(1);
    let embd = int("embedding_length").unwrap_or(0);
    let key_len = int("attention.key_length").unwrap_or(embd / heads);
    let value_len = int("attention.value_length").unwrap_or(key_len);
    let key_len_swa = int("attention.key_length_swa").unwrap_or(key_len);
    let value_len_swa = int("attention.value_length_swa").unwrap_or(value_len);
    let window = int("attention.sliding_window").unwrap_or(0) as u32;
    // Gemma's last layers reuse the KV cache of earlier ones.
    let shared = int("attention.shared_kv_layers").unwrap_or(0).min(u64::from(layers)) as u32;
    let full_interval = int("full_attention_interval").unwrap_or(0);
    let ssm_inner = int("ssm.inner_size").unwrap_or(0);
    let ssm_state = int("ssm.state_size").unwrap_or(0);

    let heads_kv = |il: u32| -> u64 {
        match get("attention.head_count_kv") {
            Some(Value::Ints(v)) => v.get(il as usize).copied().unwrap_or(0).max(0) as u64,
            Some(v) => v.int().unwrap_or(heads as i64).max(0) as u64,
            None => heads,
        }
    };
    let is_swa = |il: u32| -> bool {
        window > 0
            && match get("attention.sliding_window_pattern") {
                Some(Value::Bools(v)) => v.get(il as usize).copied().unwrap_or(false),
                // A number n: every nth layer attends to everything, the rest use the window.
                Some(v) => v.int().is_some_and(|n| n > 1 && (il as i64 + 1) % n != 0),
                None => false,
            }
    };
    let is_recurrent = |il: u32| -> bool {
        if full_interval > 0 {
            (u64::from(il) + 1) % full_interval != 0
        } else {
            // A pure state-space model (Mamba): every layer, and no attention heads.
            ssm_state > 0 && get("attention.head_count_kv").is_none() && get("attention.head_count").is_none()
        }
    };

    let mut shape = ModelShape {
        arch: arch.clone(),
        layers,
        train_context: int("context_length").unwrap_or(4096).min(u64::from(u32::MAX)) as u32,
        swa_window: window,
        ..Default::default()
    };
    for il in 0..layers.saturating_sub(shared) {
        if is_recurrent(il) {
            shape.recurrent_layers += 1;
            continue;
        }
        let kv = heads_kv(il);
        if is_swa(il) {
            shape.kv_values_swa += kv * (key_len_swa + value_len_swa);
        } else {
            shape.kv_values_full += kv * (key_len + value_len);
        }
    }
    if shape.recurrent_layers > 0 {
        // Gated delta net (Qwen3.5) and Mamba: an inner x state matrix (a head_k x head_v one per
        // value head), plus the convolution's last few inputs. Kept in f32.
        let groups = int("ssm.group_count").unwrap_or(0);
        let kernel = int("ssm.conv_kernel").unwrap_or(4);
        let state = ssm_inner * ssm_state;
        let conv = kernel.saturating_sub(1) * (ssm_inner + 2 * groups * ssm_state);
        shape.recurrent_bytes = u64::from(shape.recurrent_layers) * (state + conv) * 4;
    }
    Ok(shape)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Writes a GGUF header with the given metadata (no tensors), for tests.
    pub fn gguf(entries: &[(&str, Meta)]) -> Vec<u8> {
        let mut out = b"GGUF".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend((entries.len() as u64 + 1).to_le_bytes());
        let s = |out: &mut Vec<u8>, t: &str| {
            out.extend((t.len() as u64).to_le_bytes());
            out.extend(t.as_bytes());
        };
        // A tokenizer-sized string array, which must be skipped.
        s(&mut out, "tokenizer.ggml.tokens");
        out.extend(9u32.to_le_bytes());
        out.extend(8u32.to_le_bytes());
        out.extend(5000u64.to_le_bytes());
        for i in 0..5000 {
            s(&mut out, &format!("tok{i}"));
        }
        for (k, v) in entries {
            s(&mut out, k);
            match v {
                Meta::U32(n) => {
                    out.extend(4u32.to_le_bytes());
                    out.extend(n.to_le_bytes());
                }
                Meta::Str(t) => {
                    out.extend(8u32.to_le_bytes());
                    s(&mut out, t);
                }
                Meta::I32s(v) => {
                    out.extend(9u32.to_le_bytes());
                    out.extend(5u32.to_le_bytes());
                    out.extend((v.len() as u64).to_le_bytes());
                    for n in v {
                        out.extend(n.to_le_bytes());
                    }
                }
                Meta::Bools(v) => {
                    out.extend(9u32.to_le_bytes());
                    out.extend(7u32.to_le_bytes());
                    out.extend((v.len() as u64).to_le_bytes());
                    out.extend(v.iter().map(|&b| u8::from(b)));
                }
            }
        }
        out
    }

    pub enum Meta {
        U32(u32),
        Str(&'static str),
        I32s(Vec<i32>),
        Bools(Vec<bool>),
    }
    use Meta::*;

    /// Qwen3.5 4B as unsloth's GGUF describes it.
    pub fn qwen35_4b() -> Vec<u8> {
        gguf(&[
            ("general.architecture", Str("qwen35")),
            ("qwen35.block_count", U32(32)),
            ("qwen35.context_length", U32(262144)),
            ("qwen35.embedding_length", U32(2560)),
            ("qwen35.attention.head_count", U32(16)),
            ("qwen35.attention.head_count_kv", U32(4)),
            ("qwen35.attention.key_length", U32(256)),
            ("qwen35.attention.value_length", U32(256)),
            ("qwen35.ssm.conv_kernel", U32(4)),
            ("qwen35.ssm.state_size", U32(128)),
            ("qwen35.ssm.group_count", U32(16)),
            ("qwen35.ssm.time_step_rank", U32(32)),
            ("qwen35.ssm.inner_size", U32(4096)),
            ("qwen35.full_attention_interval", U32(4)),
        ])
    }

    /// Granite 4.1 8B: plain attention on every layer.
    pub fn granite_8b() -> Vec<u8> {
        gguf(&[
            ("general.architecture", Str("granite")),
            ("granite.block_count", U32(40)),
            ("granite.context_length", U32(131072)),
            ("granite.embedding_length", U32(4096)),
            ("granite.attention.head_count", U32(32)),
            ("granite.attention.head_count_kv", U32(8)),
        ])
    }

    #[test]
    fn hybrid_models_keep_a_kv_cache_only_on_their_attention_layers() {
        let s = parse_shape(&mut qwen35_4b().as_slice()).unwrap();
        assert_eq!(s.arch, "qwen35");
        assert_eq!(s.train_context, 262144);
        assert_eq!(s.recurrent_layers, 24);
        // 8 attention layers x 4 KV heads x (256 + 256).
        assert_eq!(s.kv_values_full, 8 * 4 * 512);
        assert_eq!(s.kv_values_swa, 0);
        // 24 layers x (32 x 128 x 128 state + 3 x 8192 conv) x 4 bytes, about 50 MB.
        assert_eq!(s.recurrent_bytes, 24 * (32 * 128 * 128 + 3 * 8192) * 4);
    }

    #[test]
    fn plain_models_keep_a_kv_cache_on_every_layer() {
        let s = parse_shape(&mut granite_8b().as_slice()).unwrap();
        assert_eq!(s.kv_values_full, 40 * 8 * 256);
        assert_eq!((s.recurrent_layers, s.recurrent_bytes), (0, 0));
    }

    #[test]
    fn sliding_window_and_shared_layers() {
        // Gemma 4 E4B: 42 layers, the last 18 reuse earlier caches, every 6th attends to everything.
        let pattern: Vec<bool> = (0..42).map(|i| (i + 1) % 6 != 0).collect();
        let bytes = gguf(&[
            ("general.architecture", Str("gemma4")),
            ("gemma4.block_count", U32(42)),
            ("gemma4.context_length", U32(131072)),
            ("gemma4.attention.head_count", U32(8)),
            ("gemma4.attention.head_count_kv", U32(2)),
            ("gemma4.attention.key_length", U32(512)),
            ("gemma4.attention.value_length", U32(512)),
            ("gemma4.attention.key_length_swa", U32(256)),
            ("gemma4.attention.value_length_swa", U32(256)),
            ("gemma4.attention.sliding_window", U32(512)),
            ("gemma4.attention.shared_kv_layers", U32(18)),
            ("gemma4.attention.sliding_window_pattern", Bools(pattern)),
        ]);
        let s = parse_shape(&mut bytes.as_slice()).unwrap();
        assert_eq!(s.kv_values_full, 4 * 2 * 1024, "layers 6, 12, 18 and 24");
        assert_eq!(s.kv_values_swa, 20 * 2 * 512);
        assert_eq!(s.swa_window, 512);
    }

    #[test]
    fn per_layer_kv_heads() {
        let bytes = gguf(&[
            ("general.architecture", Str("x")),
            ("x.block_count", U32(3)),
            ("x.attention.head_count", U32(8)),
            ("x.attention.head_count_kv", I32s(vec![8, 2, 8])),
            ("x.attention.key_length", U32(64)),
        ]);
        let s = parse_shape(&mut bytes.as_slice()).unwrap();
        assert_eq!(s.kv_values_full, (8 + 2 + 8) * 128);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse_shape(&mut b"PK\x03\x04 not a model".as_slice()).is_err());
        let mut cut = qwen35_4b();
        cut.truncate(200);
        assert!(parse_shape(&mut cut.as_slice()).is_err());
    }
}
