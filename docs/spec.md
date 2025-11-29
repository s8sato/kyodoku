# kyodoku Bot Specification

## 1. Overview

**kyodoku** is a Discord bot designed to create and remove temporary voice channels for shared reading sessions, identified by ISBN. Each ISBN corresponds to one text thread that persists across sessions, allowing readers to continue discussions asynchronously.

The project follows a *spec-first* approach — this document serves as the source of truth for future automated implementation (e.g., via Codex).

---

## 2. Goals

* Enable real-time, ephemeral voice discussions around specific books.
* Keep textual discussion logs persistent per ISBN.
* Notify users when a watched ISBN session starts.
* Maintain minimal, self-cleaning infrastructure (no manual channel management).

---

## 3. Slash Commands

### `/isbn <code> [title_override]`

* **Purpose:** Create or reuse a voice channel associated with the given ISBN.
* **Arguments:**

  * `code`: ISBN-10 or ISBN-13, with or without hyphens.
  * `title_override`: Optional manual title if metadata lookup fails.
* **Behavior:**

  * Normalize ISBN (convert ISBN-10 → ISBN-13).
  * Fetch metadata (Open Library → Google Books → fallback to override).
  * Create text thread if missing, voice channel if none exists.
  * Respond with links to both channels.
  * Voice channel auto-deletes after a period of zero members.

### `/watch <action> [code]`

* **Purpose:** Manage a personal or server-wide watchlist of ISBNs.
* **Actions:**

  * `add <code>` — subscribe to notifications when a new session starts.
  * `remove <code>` — unsubscribe from that ISBN.
  * `list` — view all watched ISBNs.

---

## 4. Data Model (PostgreSQL)

| Table            | Description                                                     |
| ---------------- | --------------------------------------------------------------- |
| `isbn`           | Stores ISBN metadata (title, subtitle, authors, source)         |
| `guild_settings` | Per-server configuration (voice category, notification channel) |
| `isbn_threads`   | Mapping of guild × ISBN to text channel ID                      |
| `watchlist`      | User and guild watch subscriptions                              |
| `voice_sessions` | Records of each active ISBN voice session                       |

---

## 5. Lifecycle

### Voice Channel Creation

1. User executes `/isbn <code>`.
2. Bot checks for existing channel → reuse or create.
3. On creation, a new `voice_sessions` record is inserted.

### Session Deletion

1. Monitor `VoiceStateUpdate` events.
2. When member count = 0, start a 2-minute grace timer.
3. If still empty, delete the channel.
4. Log `ended_at` timestamp in `voice_sessions`.

### Notifications

* When an ISBN’s voice channel reaches the configured participant threshold, the session is considered *active*.
  * Threshold defaults to **1** participant and can be overridden via the `READING_SESSION_ACTIVATION_THRESHOLD` environment variable.
* Notify all users who have that ISBN on their watchlist via DM.
  * DM message includes links to the ISBN text discussion thread (if present) and the active voice channel.

---

## 6. External Integrations

* **Metadata Sources:** Open Library (default), Google Books (fallback), manual title override as final fallback.
* **Storage:** PostgreSQL for persistence; Redis for caching and distributed locks.
* **Hosting:** Docker-based, minimal dependencies.

---

## 7. Future Extensions

* Voice recording and auto-summarization (Whisper + text summary bot).
* Integration with public reading lists (e.g., Amazon Wishlist).
* Optional web dashboard for searching ISBN sessions.

---

## 8. Non-Goals

* No persistent global state beyond ISBN metadata and text logs.
* No audio storage or moderation beyond Discord’s native tools.
* No external authentication beyond Discord OAuth2.

---

## 9. Implementation Stack (Planned)

* Language: Rust 1.76+
* Framework: Serenity + Songbird
* DB: PostgreSQL 14+
* Cache: Redis 6+
* Infra: Docker Compose for local dev

---

## 10. Milestones

| Phase | Deliverable                             | Commit Prefix      |
| ----- | --------------------------------------- | ------------------ |
| 0     | Minimal docs & spec skeleton            | `chore:` / `docs:` |
| 1     | Core slash commands (`/isbn`, `/watch`) | `feat(bot):`       |
| 2     | Auto-deletion & notifications           | `feat(session):`   |
| 3     | Metadata resilience & caching           | `refactor(meta):`  |
| 4     | Recording & summarization prototype     | `feat(summary):`   |

---

## 11. License

MIT — see `LICENSE` for details.
