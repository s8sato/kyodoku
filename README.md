# kyodoku

**kyodoku** is a minimal repository scaffold for a Discord bot that creates and removes on-demand voice channels based on ISBN codes.
It is designed for implementation by a code-generation agent (e.g. Codex) following an external specification.

## Overview

* Purpose: A *spec-driven* project starter. Contains only basic documentation and licensing.
* Language (planned): Rust
* Core stack (planned): Serenity SDK, PostgreSQL, Redis
* Spec file will be added later under `docs/spec.md`.

## Directory Structure

```plain
kyodoku/
├─ README.md          # This file
├─ LICENSE            # MIT license
├─ .gitignore         # Basic ignore rules
├─ .gitattributes     # LF normalization
├─ docs/
│  └─ README.md       # Placeholder for future spec
├─ CONTRIBUTING.md    # Guidelines for later development
└─ INIT.md            # Setup notes for maintainers
```

## License

MIT License © 2025 Shunkichi Sato

## Notes

* Implementation will begin after the specification (`/docs/spec.md`) is finalized.
* Use this repository as a **blank bootstrap**, not as a running codebase.
* Future milestones:

  * `/bot` crate (Rust)
  * `/db/migrations` for schema setup
  * CI configuration (format, lint, test)
