# Agent Development Guide

This repository follows a **spec-first** workflow.
Automated agents (e.g., Codex, Copilot, or similar) may use this guide to understand how to build from the specification.

## Responsibilities

1. Read `/docs/spec.md` as the single source of truth for behavior and structure.
2. Generate implementation code under `/bot` (Rust) according to the Serenity SDK and PostgreSQL/Redis integration described in the spec.
3. Follow conventional commit prefixes (e.g., `feat(bot):`, `fix(meta):`, `refactor(store):`).
4. Keep commits atomic and descriptive.
5. If implementation deviates from the spec, update `/docs/spec.md` and summarize the rationale in the commit message.

## Directory Targets

| Path | Purpose |
|------|----------|
| `/bot` | Rust source code (main crate) |
| `/infra/docker` | Development runtime environment |
| `/docs` | Living specification and design documents |

## Implementation Rules

- Use stable Rust (1.76+) only.
- Use `serenity` and `songbird` for Discord interaction and voice.
- Database: PostgreSQL via `sqlx`.
- Cache/Lock: Redis.
- Follow the architecture diagram in `/docs/architecture.md`.
- Generate `.env.example` files for any new services.
