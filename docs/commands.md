# kyodoku Commands Specification

## /open

- **Description:** Creates or reuses a reading session for the given ISBN.
- **Arguments:**
- `code`: ISBN-10 or ISBN-13, with or without hyphens.
- `title_override`: Optional string to use when metadata lookup fails.
- **Behavior:**
- Normalize ISBN and resolve metadata.
- Create associated text channel and voice channel.
- Return channel links.

## /watch

- **Description:** Manage ISBN watchlist.
- **Actions:**
- `list`: Show current watchlist.
- `add`: Add ISBNs to user watchlist.
- `remove`: Remove ISBNs from watchlist.
