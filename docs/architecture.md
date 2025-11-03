# kyodoku Architecture Overview

## 1. Components

| Component | Description |
|------------|-------------|
| Bot | Discord Gateway + Slash Commands handler |
| DB | PostgreSQL for persistent metadata |
| Cache | Redis for locks and ephemeral states |
| Worker | (future) STT + summarization task runner |

## 2. High-Level Flow

```plain

Discord  →  Bot (Gateway)  →  Postgres/Redis
↳  (future) Worker (STT/Summary)

```

## 3. Channel Lifecycle

1. `/isbn` invoked.
2. ISBN normalized → metadata fetched → VC created.
3. Users join VC.
4. When member count = 0 for 2 minutes, VC auto-deletes.
5. Text thread persists.

## 4. Module Breakdown (planned)

- `isbn.rs` — normalization & metadata lookup
- `routes.rs` — command dispatch
- `store.rs` — DB layer
- `util.rs` — channel helpers & deletion logic

## 5. External APIs

- Open Library (primary)
- Google Books (secondary)
- Manual override when both APIs fail
