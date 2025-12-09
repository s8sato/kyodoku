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

### Voice Channel Creation

1. User executes `/open <code>`.
2. Bot checks for existing channel → reuse or create.
3. On creation, a new `voice_channels` record is inserted.

### Channel Placement

* ISBN voice channels belong to the category provided via the `VOICE_CHANNEL_CATEGORY_ID` environment variable.
* ISBN text channels use a **configurable category ladder** with 1–9 tiers:
  * Each tier is configured via consecutive variables starting with `TEXT_CATEGORY_1_ID` (e.g., `TEXT_CATEGORY_2_ID`, `TEXT_CATEGORY_3_ID`, …). Configuration stops at the first missing variable.
  * Each category can hold up to `TEXT_CATEGORY_CAPACITY` channels (bounded between `TEXT_CATEGORY_SWAP_COUNT + max(TEXT_CATEGORY_SWAP_COUNT, TEXT_CATEGORY_PRUNE_COUNT)` and **50**).
  * Channel names exceeding the length limit are truncated with an ellipsis suffix to make the shortening visible.

### Session Deletion

1. Monitor `VoiceStateUpdate` events.
2. Start a grace timer (defaults to **1 minute** via `VOICE_CLEANUP_DELAY_SECONDS`) when a channel is first created and any time the member count returns to 0.
3. If still empty, delete the channel.
4. Log `ended_at` timestamp in `voice_channels`.

### Notifications

* When the **first participant** joins an ISBN’s voice channel, the session is considered *active*.
* Notify all users who have that ISBN on their watchlist via DM.
  * DM message includes links to the ISBN text discussion channel (if present) and the active voice channel.

### Text Channel Lifecycle (9-tier system)

* **Opening channels**
  * A brand-new ISBN channel is created at the top of **category 9**.
    * If category 9 already holds 50 channels, `/open` fails because no slots remain.
    * Existing channels in category 9 shift down one rank.
  * Reopening an existing ISBN moves it to the top of its current category; siblings shift down one rank.

* **Periodic activity evaluation** (interval controlled by `TEXT_ACTIVITY_EVAL_INTERVAL_SECONDS`)
* Each channel is scored via the configured formula; the latest score and breakdown are written to the channel topic.
  * `activity_score = watchlist_count + (unique_voice_participants × TEXT_ACTIVITY_PRESENCE_FACTOR)`
  * For categories 1 through 8, swap the bottom `TEXT_CATEGORY_SWAP_COUNT` channels in category *n* with the top `TEXT_CATEGORY_SWAP_COUNT` channels in category *n+1*.
  * Delete the bottom `TEXT_CATEGORY_PRUNE_COUNT` channels in category 9 permanently.

* **Environment variables**
* `TEXT_CATEGORY_1_ID` … `TEXT_CATEGORY_9_ID`: IDs for consecutively configured text categories (at least one required).
* `TEXT_ACTIVITY_EVAL_INTERVAL_SECONDS`: cadence for evaluating scores.
* `TEXT_ACTIVITY_PRESENCE_FACTOR`: multiplier applied to unique voice participants when computing text activity scores (default: **2.0**).
* `TEXT_CATEGORY_SWAP_COUNT`: number of channels swapped between neighboring tiers (default: **10**).
* `TEXT_CATEGORY_PRUNE_COUNT`: number of lowest-ranked channels in category 9 to delete each cycle (default: **10**).
* `TEXT_CATEGORY_CAPACITY`: maximum channel count per text category (bounded by swap/prune counts; default: **50**).

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
