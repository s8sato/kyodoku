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

1. `/open` is invoked.
2. The ISBN is normalized, metadata is fetched, and the VC/TC are created if they do not already exist.
3. Users may join the voice channel.
4. When the member count remains 0 for a certain duration, the voice channel is automatically deleted.
5. The text channel remains persistent.

## 4. Module Breakdown (planned)

- `isbn.rs` — normalization & metadata lookup
- `routes.rs` — command dispatch
- `store.rs` — DB layer
- `util.rs` — channel helpers & deletion logic

## 5. External APIs

- Open Library (primary)
- Google Books (secondary)
- Manual override when both APIs fail
