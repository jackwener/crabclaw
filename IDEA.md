# CrabClaw Idea Notes

- Source article: https://frostming.com/posts/2026/create-a-claw/
- Source date: 2026-02-12
- Document purpose: translate the author's philosophy into actionable guidance for CrabClaw.

## Executive Summary

The article argues that AI systems are moving from:

1. `Chatbot era (1.0)`: one request, one inference.
2. `Agent era (2.0)`: multi-step tool orchestration with stronger framework control.
3. `AI native era (3.0)`: a minimal core where AI can shape its own capabilities with less hard-coded framework behavior.

The key claim is not "no engineering," but "smaller mandatory framework + more model-driven evolution."

## What Changed in the Author's Thinking

The author starts from a practical replication goal: add messaging capability and reconstruct OpenClaw-like behavior on top of Bub.  
Early implementation follows familiar engineering patterns: handlers, adapters, and integration layers. This worked, but produced a recurring friction:

- every new interaction requirement required framework edits
- framework-owned capabilities became rigid compared to AI-created capabilities
- channel-specific logic risked becoming a permanent control layer over the agent

The turning point: treat framework capabilities as temporary prosthetics rather than permanent organs.

## Core Philosophy Extracted

1. **Minimize forced behavior**
   - Do not hard-code more behavior than necessary for safety and determinism.
   - Keep the core runtime narrow.
2. **Prompt as primary control surface**
   - Feature growth should happen mostly via prompt-driven instructions and skill text.
   - Human operators control through natural language intent rather than framework patching.
3. **Framework as capability kernel, not feature warehouse**
   - Keep only irreducible primitives in the kernel.
   - Avoid embedding every convenience directly into framework code.
4. **AI-managed evolution**
   - Prefer a system that can produce and evolve its own operating artifacts (scripts, skills, procedures).
   - Human role shifts from micromanaging implementation to setting goals and constraints.
5. **Channel adapters are wake-up paths, not the brain**
   - Messaging integration should trigger and route work.
   - Higher-level decisions should remain in the agent loop.

## Engineering Interpretation for CrabClaw

This philosophy does **not** imply abandoning engineering quality.  
For CrabClaw, it means a strict split:

- **Kernel (must be deterministic and testable)**
  - command boundary and routing rules
  - session/tape semantics
  - execution safety and error categories
  - reproducible startup contract
- **Evolvable layer (can be model-driven)**
  - skill content
  - workflow recipes
  - optional channel behavior details

In short: harden the kernel, soften the outer behavior layer.

## Design Constraints Derived for CrabClaw

1. Keep the Rust core small and explicit.
2. Introduce built-in tools only when they improve safety, observability, or determinism.
3. Prefer text-defined skills over adding bespoke framework code for every new feature.
4. Ensure every mandatory runtime behavior is auditable and can be explained post hoc.
5. Document every philosophy-breaking framework expansion in ADRs.

## Risks and Countermeasures

1. **Risk: over-minimalism can hide missing operational guarantees**
   - Countermeasure: preserve strict test plans and review gates for kernel behavior.
2. **Risk: model autonomy can create non-repeatable behavior**
   - Countermeasure: keep append-only tape and structured command/result records.
3. **Risk: "AI will handle it" can dilute code quality**
   - Countermeasure: require deterministic acceptance criteria before shipping changes.
4. **Risk: channel integrations can become implicit business logic**
   - Countermeasure: keep channel code as transport adapters only.

## Practical Roadmap Mapping

1. Build a minimal `library+CLI` kernel first.
2. Implement deterministic routing and tape semantics before advanced integrations.
3. Add a startup protocol that allows one-shot execution orchestration.
4. Move feature expansion to skill and prompt patterns wherever possible.
5. Keep reviewer parity checks against Bub for behavior and architecture, not syntax-level translation.

## What "Aligned with Bub Philosophy" Means

CrabClaw is aligned when:

1. it preserves deterministic core behavior,
2. it avoids unnecessary framework bloat,
3. it enables prompt-driven capability growth,
4. it treats AI as an autonomous operator inside explicit safety boundaries.
