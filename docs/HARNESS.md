# The harness: small models, small contexts, long tasks

"The entire OS is the harness." A 4B model can't hold a long task in its head, but it can follow one small step at a time if something else keeps the plan, notices when it's stuck, and keeps the context from filling up. This note sums up what other agents do, what llama.cpp offers on machines without a graphics card, and what Tripppin does. Written November 2026 against llama.cpp **v1.0.0** (the tag pinned in `os/Containerfile`); every flag below was checked in that tag's `tools/server/README.md` and source.

## 1. How other harnesses stay on track

| Project | Planning | Context management | Memory across sessions |
|---|---|---|---|
| [Claude Code](https://docs.anthropic.com/en/docs/claude-code) | A todo list tool (`TodoWrite`): the model writes short items, marks one in progress, ticks them off; the list is re-shown to it | Auto-compaction near the context limit: the conversation is summarised and the summary plus todos carry on; `/compact` by hand | `CLAUDE.md` files read at start; sub-agents get fresh contexts |
| [Codex CLI](https://github.com/openai/codex) | `update_plan` tool: steps with pending / in_progress / completed, exactly one in progress | Auto-compaction by token count, summary replaces history | `AGENTS.md` |
| [Aider](https://aider.chat/docs/repomap.html) | None built in (architect/editor split instead) | A token-budgeted "repo map" instead of whole files; chat history summarised by a weaker model when it grows | Conventions files |
| [OpenHands](https://docs.all-hands.dev/) | Task tracker tool | Pluggable [condensers](https://github.com/All-Hands-AI/OpenHands/tree/main/openhands/memory/condenser): drop observations, keep first/last N events, LLM summary of the middle | Microagents (markdown) |
| [smolagents](https://huggingface.co/docs/smolagents/tutorials/inspect_runs) | `planning_interval`: every N steps the model writes facts known/needed and an updated plan | Memory steps can be pruned by callbacks | none |
| [Goose](https://block.github.io/goose/docs/guides/sessions/smart-context-management) | Todo extension | Auto-compact at a threshold (default 80 %), or truncate / summarise | `.goosehints`, memory extension |
| [opencode](https://opencode.ai/docs/) | Todo tool | Auto-compact near the limit; old tool outputs "pruned" first | `AGENTS.md` |

What they agree on, and what the research says:

- **An explicit, short plan that's re-shown every turn**, with one step "in progress". Plan-then-act beats pure ReAct for multi-step work ([Plan-and-Solve, Wang et al. 2023](https://arxiv.org/abs/2305.04091); [ReAct, Yao et al. 2022](https://arxiv.org/abs/2210.03629)); re-planning on failure helps more for weak models than for strong ones.
- **Put the state the model needs at the end of the prompt.** Models use the start and end of a long context best and the middle worst ([Lost in the Middle, Liu et al. 2023](https://arxiv.org/abs/2307.03172)); small models degrade fastest with length ([RULER, Hsieh et al. 2024](https://arxiv.org/abs/2404.06654)). So keep the context short and the notebook last.
- **Compaction by stages, cheapest first:** old tool output goes before decisions, decisions before requests; the plan and notes are never compacted. Everyone triggers on real token counts with headroom (Goose 80 %, Codex/Claude Code close to the limit), and compacts well below the trigger so it doesn't run every turn.
- **Memory outside the context window** (MemGPT's tiered memory, [Packer et al. 2023](https://arxiv.org/abs/2310.08560); the `*.md` files above): a notebook that survives compaction and restarts.
- **Loop detection** is mostly ad hoc: step caps, refusing identical repeated calls (OpenHands' stuck detector looks for repeated action/observation pairs), and forcing a final answer.

LLM-written summaries are the norm in cloud agents, but on a CPU a summary costs a full prompt read plus hundreds of generated tokens, and a 4B model's summaries lose details. Tripppin's notebook is written by the harness (one line per step, generated from the record), so compaction costs nothing and never hallucinates.

## 2. llama.cpp on CPUs and integrated GPUs

**KV cache size.** Per token it's `layers_with_kv × kv_heads × (key_len + value_len) × bytes`. That depends heavily on the architecture, so Tripppin reads it from the GGUF header (`crates/hwdetect/src/gguf.rs`):

| Model (our catalog) | Layers with a full KV | f16 per token | 32k tokens at q8_0 |
|---|---|---|---|
| Qwen3.5 4B / 9B (hybrid: 3 of 4 layers are linear attention with a fixed ~50 MB state) | 8 | 32 KiB | 0.53 GiB |
| Qwen3.5 35B-A3B | 10 | 20 KiB | 0.33 GiB |
| Gemma 4 E4B (5 of 6 layers sliding-window 512, last 18 share KV) | 4 + small window | 16 KiB | 0.27 GiB + 40 MB |
| Granite 4.1 8B (plain attention) | 40 | 160 KiB | 2.7 GiB |

**Flags that matter (all in v1.0.0):**

- `--cache-type-k/-v q8_0` halves the cache with no measurable quality loss; quantised V needs `--flash-attn on` (llama.cpp turns FA on itself in `auto`, errors if it's `off`). q4_0 hurts quality noticeably; not used.
- **Prompt caching** (`cache_prompt`, on by default) reuses the longest common prefix with the previous request. For plain attention any prefix works. For **hybrid/recurrent (Qwen3.5) and sliding-window (Gemma) models the cache can't be rewound to an arbitrary token**: the server can only go back to a *context checkpoint* ([PR #15293](https://github.com/ggml-org/llama.cpp/pull/15293)). v1.0.0 makes checkpoints at the start of user messages (spaced `--checkpoint-min-step`, default 8192, apart, but always at the last user message) and just before the end of the prompt. If the prompt changes anywhere else, the whole conversation is read again. **This is the single biggest speed issue for an agent on a CPU**, and the harness has to be shaped around it (see §3).
- `--ctx-checkpoints N` (default 32): each checkpoint holds the recurrent state / window (~50–60 MB for Qwen3.5), kept in RAM. 8–16 is plenty.
- `--cache-ram MiB` (default 8192!): a host-RAM cache of whole other conversations, so switching between the orb and a terminal session doesn't start over ([PR #16391](https://github.com/ggml-org/llama.cpp/pull/16391)). 8 GiB is too much on a 16 GB laptop; we cap it at 1–2 GiB.
- `--cache-reuse N` shifts matching chunks after the middle of the prompt changed (after a compaction). Needs a cache that can shift; the server turns it off for hybrid/SWA models.
- **Context shift** (`--context-shift`, off by default) drops old tokens when the context fills. It cuts tool calls in half; the harness compacts instead. `--keep` only matters with it.
- `--swa-full` keeps a full-size cache for SWA layers; only useful to make every prefix reusable, at a large memory cost. Not used.
- `--kv-unified` / `--parallel`: one slot (`--parallel 1`) gives the whole context to one conversation.
- `-nkvo/--no-kv-offload` keeps the cache in system RAM while layers run on the GPU: attention then runs on the CPU, slow. On an iGPU it saves nothing (same RAM). Not used.
- Loading: `--mlock`/`--no-mmap` became `--load-mode {auto,mmap,mlock,none}` in v1.0.0. Default mmap keeps CPU weights in the page cache, which the kernel can drop under pressure (slower, never OOM). mlock on a 16 GB desktop would pin gigabytes. Not used.
- **Slot save/restore** (`--slot-save-path`, `POST /slots/0?action=save`) writes a conversation's cache to disk, including checkpoints. Useful later for resuming sessions after `--sleep-idle-seconds` unloads the model; not wired up yet.
- **`--fit on`** (default) does a dry run and lowers *unset* settings (GPU layers, context) to fit each device's free memory, leaving `--fit-target` MiB (default 1024). It never touches settings you set; it counts host memory only when there's no GPU.

**When memory runs out.** CPU: llama.cpp allocates the KV cache up front, so a too-big context fails at start or pushes the desktop into swap/OOM (our unit has `OOMScoreAdjust=800`, so the model dies first). Dedicated GPU: allocation fails, or with `--fit` fewer layers go to the GPU. Integrated GPU: Vulkan allocations come from the GTT, pinned system RAM; past the limit allocation fails, and near it the desktop's own graphics starve.

**AMD integrated GPUs (UMA).** The Radeon 680M/780M/890M share system RAM. The GPU may map the BIOS carve-out (`mem_info_vram_total`, 0.5–8 GB) plus the GTT (`mem_info_gtt_total`), which is half the RAM by default since the kernel moved GTT onto TTM's page limit (raise with the `ttm.pages_limit=` kernel argument; `amdgpu.gttsize` is deprecated) ([amdgpu module parameters](https://docs.kernel.org/gpu/amdgpu/module-parameters.html)). llama.cpp's Vulkan backend reports a UMA device's memory as the sum of its heaps (`ggml_backend_vk_get_device_memory`). Speed: generation is memory-bound and about the same on CPU and iGPU; **prompt reading is several times faster on the iGPU** (llama.cpp's [Vulkan scoreboard](https://github.com/ggml-org/llama.cpp/discussions/10879): a 780M reads a 7B Q4 prompt at a few hundred tokens/s, a Zen 4 laptop CPU at well under 100), and an agent spends most of its time reading tool output. So: use the iGPU when the whole model fits its share, otherwise the CPU (partial offload on UMA saves no memory and adds copies). Pass `--device none` for the CPU so Mesa's software Vulkan (llvmpipe) isn't picked up.

## 3. What Tripppin does

**Serving** (`crates/hwdetect/src/serving.rs`, `trip model serve`):

1. Read the model's shape from its GGUF header.
2. Memory budget = RAM − max(4 GiB, ¼ RAM) for the desktop, minus the prompt cache (1 GiB, 2 GiB from 24 GB up) and checkpoints (8 or 16).
3. GPU: a graphics card → `--fit on --fit-target 1024`, llama.cpp places the layers. An integrated AMD/Intel GPU → if the model, a full cache and compute buffers fit in (carve-out + GTT − 2 GiB) → `--fit on --fit-target 2048`; else CPU. No GPU → CPU (`--device none -ngl 0 --fit off`).
4. Context = the longest multiple of 1024, between 8192 and the tier's context (and the model's trained length), whose q8_0 cache fits what's left. If the weights alone fill the budget (35B-A3B on 32 GB, mmapped), the cache gets at most ⅛ of the budget.
5. Always: `--flash-attn on --cache-type-k q8_0 --cache-type-v q8_0 --ctx-checkpoints N --cache-reuse 256 --cache-ram N --no-context-shift --parallel 1`. The reason is logged ("integrated GPU, 8.0 GiB of shared memory usable; …"). `TRIPPPIN_LLAMA_ARGS` still overrides anything.

Resulting arguments on the owner's kind of machine (AMD laptop, no graphics card), with the recommended tier:

- **16 GB** (Qwen3.5 4B, iGPU with ~8 GiB GTT+carve-out): `--ctx-size 32768 --flash-attn on --cache-type-k q8_0 --cache-type-v q8_0 --ctx-checkpoints 8 --cache-reuse 256 --cache-ram 1024 --no-context-shift --fit on --fit-target 2048` (≈ 3.9 GiB when full).
- **32 GB** (Qwen3.5 35B-A3B, 20.5 GiB, more than the iGPU's ~15 GiB): `--ctx-size 16384 … --ctx-checkpoints 16 --cache-reuse 256 --cache-ram 2048 --no-context-shift --device none --n-gpu-layers 0 --fit off` (CPU; MoE with 3B active is still quick).

**Harness** (`crates/trip/src/agent.rs`, `session.rs`):

- **Plan tool.** `plan {steps: [...]}` writes or rewrites a plan of at most 12 short steps (a list or a numbered string both work); `done {step, check}` marks one done with how it was checked and answers with the next step. The notebook shows the plan with `[done] / [now] / [ ]` and "work only on step N". The plan lives in the `Session` (saved as JSON), so it survives compaction and `trip shell --continue`; an unfinished plan carries over to the next request ("keep going"), a finished one is cleared. A request may take 40 steps, or 12 per plan step up to 150.
- **Stuck detection.** Identical calls are refused after 2 (exec) or 3 (other tools, not job polling). After 3 tool steps with no plan, the notebook asks for one. 8 steps without the plan moving → a reminder to finish, re-plan or ask; 3 failures in a row → "don't try the same thing again". 16 steps without progress or 5 failures in a row → the next request goes out **without tools**, so the model has to say where it got to and ask.
- **Cache-friendly prompts.** The notebook goes in its own user message after the conversation instead of being pasted into the newest message. The conversation before it is then byte-for-byte the same from one step to the next, and the server's checkpoint at "the last user message" sits exactly where the new part begins: each step reads only the newest call, its output and the notebook, even on Qwen3.5. (Right after a request, the notebook joins it, so roles still alternate.)
- **Compaction on real token counts.** The context size comes from `/props`; after every reply the server's `usage` turns characters into tokens for this model. Compaction starts at 85 % of (context − 2,304 reserved for the reply) and goes down to 55 %, so it runs rarely and the cache is only rebuilt then. Stages: earlier requests' steps → old outputs → the current request's oldest steps (two always stay) → whole earlier requests (the notebook lists the last three) → the newest output cut to its end. Each compaction is recorded in `Session.dropped` (tokens before/after, every dropped step and request) and in the session's Markdown.
- **UI events.** `waiting("loading the model")` when `/props` says the server is asleep or starting; `waiting("reading 3,200 tokens (40%)")` from llama.cpp's `return_progress`; `waiting("writing")` at the first token; `waiting("running <cmd>")` during commands. `usage(...)` after every reply with the server's prompt, cached and completion tokens, context size, tokens/s and session totals (llama.cpp's `timings` or the OpenAI `usage` from cloud providers, via `stream_options.include_usage`). `plan(...)` whenever the plan changes.

**Not done yet:** saving slots to disk so a session resumes warm after the idle unload; measuring the CPU-vs-iGPU crossover on real APUs (the rule above is "whole model fits"); a per-machine override in Settings (today `TRIPPPIN_LLAMA_ARGS`); evals of the plan tool with each small model.
