# kyodoku Commands Specification

## /open

- **Description:** Creates or reuses a reading session for the given ISBN.
- **Arguments:**
- `code`: ISBN-10 or ISBN-13, with or without hyphens.
- **Behavior:**
- Normalize ISBN and resolve metadata (Google Books first, then Open Library).
- Create associated text channel and voice channel.
- Return channel links.
- If metadata cannot be resolved, respond with guidance that it may be a new or unreleased title and encourage Open Library contributions.

## /watch

- **Description:** Manage ISBN watchlist.
- **Actions:**
- `list`: Show current watchlist.
- `add`: Add ISBNs to user watchlist.
- `remove`: Remove ISBNs from watchlist.
