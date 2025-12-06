# Documentation

This directory houses the living specification and design notes for **kyodoku**. It is the primary reference for agents and
contributors implementing features in the bot crate.

## Contents

- `spec.md` — full product specification and workflow overview.
- `commands.md` — concise slash-command contract for Discord reviewers.
- `architecture.md` — component overview and lifecycle notes.
- `setup_discord.md` — step-by-step guide for running the bot in a local Discord server.
- `server_configuration.md` — recommended Discord channel structure for kyodoku servers.
- `deployment_oracle.md` — CI-driven deployment guide for Oracle Cloud Free Tier.
- `deployment_fly.md` — Fly.io Machines deployment guide and app configuration template.

All project changes should be validated against these documents. Update the relevant file when the implementation diverges from
the agreed behaviour.
