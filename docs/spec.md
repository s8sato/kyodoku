# kyodoku Bot Specification

## 1. Overview

**kyodoku** is a Discord bot designed to create and remove temporary voice channels for shared reading sessions, identified by ISBN. Each ISBN corresponds to one text channel that persists across sessions, allowing readers to continue discussions asynchronously.

The project follows a *spec-first* approach — this document serves as the source of truth for future automated implementation (e.g., via Codex).

---

## 2. Goals

* Enable real-time, ephemeral voice discussions around specific books.
* Keep textual discussion logs persistent per ISBN.
* Notify users when a watched ISBN session starts.
* Maintain minimal, self-cleaning infrastructure (no manual channel management).

---

## 3. Slash Commands

### `/open <code>`

* **Purpose:** Create or reuse a voice channel associated with the given ISBN.
* **Arguments:**

  * `code`: ISBN-10 or ISBN-13, with or without hyphens.
* **Behavior:**

  * Normalize ISBN (convert ISBN-10 → ISBN-13).
  * Fetch metadata (Google Books → Open Library).
  * When metadata resolution fails, return an error noting it may be a pre-release or newly published title and encourage contributions to Open Library.
  * Create text channel if missing, voice channel if none exists.
  * Respond with links to both channels.
  * Voice channel auto-deletes after a period of zero members.

### `/watch <action> [code]`

* **Purpose:** Manage a personal or server-wide watchlist of ISBNs.
* **Actions:**

  * `add <code>` — subscribe to notifications when a new session starts.
  * `remove <code>` — unsubscribe from that ISBN.
  * `list` — view all watched ISBNs.
* **Limits:** `WATCHLIST_LIMIT` (default: **30**) caps how many ISBNs a user can watch per guild. Attempts to exceed the cap
  return an error.

---

## 4. Data Model (PostgreSQL)

| Table            | Description                                                     |
| ---------------- | --------------------------------------------------------------- |
| `books`           | Stores ISBN metadata (title, subtitle, authors, source)         |
| `guilds` | Per-server configuration (notification channel) |
| `text_channels`   | Mapping of guild × ISBN to text channel ID                      |
| `watchlist`      | User and guild watch subscriptions                              |
| `voice_channels` | Records of each active ISBN voice session                       |

---

## 5. Lifecycle

### Channel Budget & Layout Constraints

* Server-wide channel usage is bounded by two configurable caps:
  * `MAX_SERVER_CHANNELS` (default: **480**) — safety ceiling to keep the server comfortably under Discord’s 500-channel limit.
  * `TEXT_CHANNEL_TOTAL_BUDGET` (default: **430**) — ceiling for all text channels across categories so operational headroom remains under the 450 target.
* Text channels always live inside managed categories so they appear visually below operational and voice categories.
  * A pool of text categories is created with the prefix from `TEXT_CATEGORY_PREFIX` (default: `isbn-text`) and anchored beneath the voice category via `TEXT_CATEGORY_POSITION_BASE` (default: a value lower than voice categories).
  * Each category is capped at **50** channels. When the active category reaches the cap, a new category in the pool is created and used for subsequent text channels.
* Channel creation first attempts to free quota (archiving or deleting stale channels) before rejecting an `/open` request. If quotas cannot be satisfied after cleanup, the bot returns an informative error mentioning which cap would be exceeded.

### Voice Channel Creation

1. User executes `/open <code>`.
2. Bot checks for existing channel → reuse or create.
3. On creation, a new `voice_channels` record is inserted.

### Channel Placement

* ISBN text channels belong to the managed text category pool and are always placed below voice-related categories for visual separation.
* ISBN voice channels belong to the category provided via the `VOICE_CHANNEL_CATEGORY_ID` environment variable.
* Channel names exceeding the length limit are truncated with an ellipsis suffix to make the shortening visible.
* Newly created or recently active text channels are moved to the top of the first text category in the pool to keep them prominent.

### Session Deletion

1. Monitor `VoiceStateUpdate` events.
2. Start a grace timer (defaults to **1 minute** via `VOICE_CLEANUP_DELAY_SECONDS`) when a channel is first created and any time the member count returns to 0.
3. If still empty, delete the channel.
4. Log `ended_at` timestamp in `voice_channels`.

### Notifications

* When the **first participant** joins an ISBN’s voice channel, the session is considered *active*.
* Notify all users who have that ISBN on their watchlist via DM.
  * DM message includes links to the ISBN text discussion channel (if present) and the active voice channel.

### Text Channel Archiving

* A dedicated `ARCHIVED_CHANNEL_CATEGORY_ID` holds overflow text channels.
* `ACTIVE_TEXT_CHANNEL_CAPACITY` (default: **150**) limits how many ISBN channels remain in the active pool before overflow moves to the archive category.
* A background task runs every `ARCHIVE_POLL_INTERVAL_SECONDS` (default: **86400** seconds) to:
  * Move channels beyond the configured capacity into the archived category.
  * Ignore channels that have messages within `TEXT_CHANNEL_ACTIVITY_LOOKBACK_SECONDS` (default: **604800** seconds = 7 days) so recently active threads are not penalized.
  * Persist archive metadata (archived time, expiration, and original category) in the database and compute expiration from the stored timestamp and `ARCHIVE_RETENTION_SECONDS` (default: **5184000** seconds = 60 days).
  * Sort candidates by recent message activity so channels with sustained discussion are archived last, minimizing churn for active threads.
  * Delete archived channels whose grace period has elapsed.
  * Optionally refresh the channel topic with `format_archive_topic` for user guidance; topics may be edited freely because state is sourced from the database.
* Archived channel topics present timestamps in the configured `TIME_ZONE` (IANA name, default: `UTC`).
* When `/open` is executed for an ISBN whose channel is archived, the channel is moved back to the top of the text category, and its archived record is deleted so the channel becomes active again.
* Imminent deletions are announced before the channel is removed:
  * `ARCHIVE_DELETE_NOTICE_LEAD_SECONDS` (default: **259200** seconds = 72 hours) defines the minimum lead time between the first deletion warning and removal.
  * The warning is posted in-channel and DM’d to watchlist subscribers so they can act; both messages include the scheduled deletion time and a quick “Extend” button that resets the expiration by `TEXT_CHANNEL_EXTENSION_SECONDS` (default: **604800** seconds = 7 days).
  * Running `/open` for the channel’s ISBN also cancels the pending delete and returns the channel to the top of the active pool; this is one of the supported extension paths.
* When capacity is constrained (text or total channel budget), the archiver preemptively queues the stalest archived channels into the notice window to free space while honoring the deletion grace period.

### Command Intake Moderation

* When `COMMAND_INPUT_CHANNEL_ID` is configured, the bot watches that text channel and immediately deletes any non-command messages posted by non-admin members to keep the slash-command entry point clean.

### Bot Visibility

* To avoid unintended invites, the Discord application should have the **Public Bot** toggle disabled in the Developer Portal.

---

## 6. External Integrations

* **Metadata Sources:** Google Books (default), Open Library (fallback).
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
| 1     | Core slash commands (`/open`, `/watch`) | `feat(bot):`       |
| 2     | Auto-deletion & notifications           | `feat(session):`   |
| 3     | Metadata resilience & caching           | `refactor(meta):`  |
| 4     | Recording & summarization prototype     | `feat(summary):`   |

---

## 11. License

MIT — see `LICENSE` for details.
