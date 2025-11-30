# GitHub Actions deployment to Oracle Cloud Free Tier

This document describes the CI pipeline that builds and deploys the kyodoku bot container to an Oracle Cloud Free Tier Compute
instance via GitHub Actions. The workflow produces a multi-architecture image compatible with Ampere A1 (ARM64) and standard
AMD64 shapes.

## Prerequisites

1. **Oracle Cloud tenancy setup**
   - Create an Oracle Container Registry (OCIR) repository (e.g., `iad.ocir.io/<tenancy-namespace>/kyodoku`).
   - Generate an *Auth Token* for a user with push/pull access to the repository. Use `tenancy-namespace/username` as the OCIR
     username when logging in via Docker.
   - Provision a Free Tier Compute instance with Docker Engine and Docker Compose installed. Open outbound HTTPS so the host can
     pull images from OCIR.

2. **Runtime configuration on the Compute instance**
   - Create `/opt/kyodoku` on the VM and copy `infra/docker/docker-compose.deploy.yml` there.
   - Add a `bot.env` file alongside it, derived from [`bot/.env.example`](../bot/.env.example). This file is not committed to
     the repository and must contain production values for PostgreSQL/Redis connections and Discord credentials.
   - The deploy compose file now provisions PostgreSQL and Redis on the same VM. Database files persist in the `db-data` Docker
     volume; ensure the VM has sufficient disk and that `/var/lib/docker` is not cleared between reboots.

3. **GitHub Secrets**
   Set the following secrets in the repository so the workflow can push images and connect to the VM:

   | Secret | Purpose |
   | --- | --- |
   | `OCIR_REGION` | Region prefix (e.g., `iad`); used to build the OCIR registry URL. |
   | `OCIR_REPO` | Full repository path without tag (e.g., `iad.ocir.io/<tenancy-namespace>/kyodoku`). |
   | `OCIR_USERNAME` | `tenancy-namespace/username` for OCIR. |
   | `OCIR_PASSWORD` | Auth Token for the OCIR user. |
   | `OCI_HOST` | Public hostname or IP of the Compute instance. |
   | `OCI_SSH_USER` | SSH username on the VM. |
   | `OCI_SSH_KEY` | Private key (PEM) for SSH access. |
   | `OCI_SSH_PORT` | (Optional) SSH port if not `22`. |

## Workflow overview

The workflow is defined in [`.github/workflows/oci-deploy.yml`](../.github/workflows/oci-deploy.yml):

1. **Build and push**
   - Checks out the repository and sets up QEMU + Buildx for multi-architecture builds.
   - Logs in to OCIR and builds the bot image from `infra/docker/Dockerfile.bot` for `linux/amd64` and `linux/arm64`.
   - Pushes tags `${OCIR_REPO}/kyodoku-bot:<commit-sha>` and `:latest` to OCIR.

2. **Deploy**
   - Copies `infra/docker/docker-compose.deploy.yml` to `/opt/kyodoku` on the target VM (creating the directory if needed).
   - Logs in to OCIR on the VM, pulls the built image, and runs `docker compose -f docker-compose.deploy.yml up -d` with
     `KYODOKU_IMAGE` set to the tag from the build job (or a manually supplied tag). This brings up the bot alongside local
     PostgreSQL and Redis containers.

## How to run the deployment

- **Automatic on main**: pushes to `main` build, push, and deploy the latest commit.
- **Manual dispatch**: trigger “Deploy kyodoku to Oracle Cloud” from the Actions tab. Optionally supply `image-tag` to redeploy an
  already published image.

## Operational notes

- Ensure the VM retains `bot.env` across restarts; the workflow does not provision or overwrite it.
- To rotate credentials, update `bot.env` on the VM and rerun the workflow. Secrets used by the workflow can be rotated in the
  repository settings without changing the pipeline code.
- If a rollback is required, re-run the workflow with `image-tag` set to a previously published SHA.
