# Contributing Guidelines

This repository is intentionally minimal.
Implementation will start **after** the specification (`/docs/spec.md`) has been finalized.

## Principles

- **Spec-first** development: all functionality must trace back to written requirements.
- **Reproducibility:** code and infra should be fully reproducible from documentation.
- **Simplicity:** avoid premature abstraction; start with a minimal viable structure.

## Workflow

1. Discuss or draft changes in `/docs/spec.md` or related design documents.
2. Once approved, implement in a dedicated branch.
3. Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for all commit messages.
4. Follow `rustfmt` defaults for code formatting (after implementation begins).

## Licensing

All contributions will be licensed under the MIT License.
