# Initialization Notes

This repository is a **skeleton** intended for spec-driven implementation by a code-generation agent (e.g., Codex).

## Current State

- Contains only metadata and minimal documentation.
- No source code, CI configuration, or infrastructure files are present.

## Next Steps

After the main specification (`/docs/spec.md`) is complete, add:

1. `/bot` crate — Rust Discord bot implementation (Serenity SDK)
2. `/db/migrations` — PostgreSQL schema migrations
3. CI pipeline — linting, formatting, and testing
4. Release and deployment workflow (optional)

## Guideline for Agents

When an automated system (like Codex) initializes development:

- Read `/docs/spec.md` as the source of truth.
- Use the stack defined therein (Rust + PostgreSQL + Redis).
- Commit generated files incrementally with clear change summaries.
