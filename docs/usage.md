# Asobi: Usage Guide

This is Asobi's interface reference: what each command does, what it accepts, and what it returns. It describes the CLI and nothing more.

It describes command behavior, not agent workflow: when to read the graph, what to write at closeout, or how to sequence a task board. That guidance differs between users and lives in the [`asobi` skill](https://github.com/azusachino/harus-skills/blob/main/skills/asobi/SKILL.md), which cites this document for exact contracts.

## For humans

### Installation

From crates.io (Rust 1.85+ toolchain required for edition 2024):

```bash
cargo install asobi                         # local-only CLI
cargo install asobi --features remote      # CLI with remote mode
cargo install asobi-server                 # named-graph HTTP server
```

For a prebuilt release, install the CLI and server binaries from the platform archive; the release CLI includes remote support. A source install can use `cargo install --git https://github.com/azusachino/asobi asobi --features remote`.

Prebuilt binaries are available in the [GitHub release archive](https://github.com/azusachino/asobi/releases); the platform archive contains the remote-enabled `asobi` CLI and `asobi-server`.

Or build locally:

```bash
git clone https://github.com/azusachino/asobi && cd asobi
make build            # graph CLI at ./target/debug/asobi
```

### Shell completion

Generate completions from the installed binary so the script always matches the version of Asobi being used:

```bash
# zsh
mkdir -p ~/.zfunc
asobi completions zsh > ~/.zfunc/_asobi
# Add this once before compinit in ~/.zshrc:
# fpath=(~/.zfunc $fpath)

# bash
mkdir -p ~/.local/share/bash-completion/completions
asobi completions bash > ~/.local/share/bash-completion/completions/asobi

# fish
asobi completions fish > ~/.config/fish/completions/asobi.fish
```

The command also supports `elvish` and `powershell`. Completions cover commands, flags, enum values, and help text; entity names remain dynamic graph data and are intentionally resolved through `search` rather than a stale completion cache.

### Upgrade to 0.8

Asobi 0.8 starts a new graph and refuses pre-0.8 graph files without modifying them. Move an old local graph aside before running 0.8; server-side graph files created by an older version must likewise be moved aside on the server. There is no automatic migration or import.

The former session handoff is replaced by graph-backed tasks. `session` is no longer a special type; legacy session entities are ordinary entities. Skills management and `[skills]` configuration are removed; any existing `[skills]` block is ignored. Install the maintained skill with `npx skills add https://github.com/azusachino/harus-skills --skill asobi --agent universal`.

### Workspace setup

Run once on a new machine — defaults to user-level XDG paths:

```bash
asobi init
# created  ~/.local/share/asobi/data
# created  ~/.local/share/asobi/topics
# created  ~/.local/share/asobi/config
```

The user-level workspace is a single `$XDG_DATA_HOME/asobi/` root (default `~/.local/share/asobi/`) holding the same `{data,config,topics,caches}` subtree as a project-local `.asobi/`. `XDG_DATA_HOME` is honored on every platform — macOS included. No root or elevation needed: it lives inside `$HOME` and is owned by the invoking user.

To keep a project's graph isolated and checked in alongside the code, use the local layout:

```bash
cd ~/code/my-project
asobi init --local
# writes ./asobi.toml + ./.asobi/{data,topics,config}/
```

`asobi.toml` (project-local mode):

```toml
data_dir   = ".asobi/data"
config_dir = ".asobi/config"
topics_dir = ".asobi/topics"
```

Path resolution order at runtime: project-local `asobi.toml` → project-local `.asobi/` → XDG. Both `init` modes are idempotent.

Add `.asobi/` to `.gitignore`; the `asobi.toml` itself can be checked in.

### Common workflows

**Recall stored state by truth or by name:**

```bash
asobi search --where status=IN_PROGRESS
asobi show "my-project:task:deploy"
```

**Store a decision (supports hierarchical naming and seeded observations):**

```bash
asobi new "project-x:architecture" "project" --obs "Switched from serde_yaml to toml crate — better error messages"
```

**Link related concepts (preserves case and dots):**

```bash
asobi new "UserPreferences" "preference"
asobi new "CLAUDE.md" "reference"
asobi link "project-x" "UserPreferences" "follows"
```

**Search (supports SQLite FTS5, segment matching, and truth filters):**

```bash
asobi search "tokio"           # finds "tokio" and "tokio-util"
asobi search "mobile"          # finds "ame:mobile-support:task-1" (segment match)
asobi search "auth*"           # prefix: matches "auth", "authentication", "authorize"
asobi search "async AND error" # both words must appear
asobi search "deploy OR ship"  # either word
asobi search "auth" --limit 25 # override the default of 10 matched nodes
asobi search --where status=READY # find all entities with status truth set to READY
asobi search "bug" --where status=READY --where priority=high # filter by multiple truths AND the query
```

Use `graph` when the whole graph is wanted. `search` is intentionally top-K by default so a broad term does not accidentally return it.

**Persist state — truths for the current value, observations for the trail:**

```bash
asobi truth "my-project:task:search" "status" "DONE"
asobi truth "my-project:task:search" "next" "implement FTS5 index"
asobi obs "my-project:task:search" "completed 2026-05-21: added the FTS5 index"
```

A truth is the right home for anything read back as _current_ state, because writing the same key updates it in place. Observations accumulate and are evicted at the cap, so a next-action stored as an observation can silently age out.

### Lifecycle

Three rules, and no others:

1. **Durable entities live forever** — `project`, `concept`, `reference`, `preference`, `standard`. Each keeps its most recent 200 observations; older ones are evicted as new ones arrive.
2. **An open task idle for 7 days is abandoned** — its status becomes `ABANDONED`, with an observation recording that it happened automatically. A task with no `status` truth counts as open. An epic is protected while any `part_of` child is still open: an epic's own entity goes quiet while its children are worked.
3. **Finished tasks are deleted after 7 more days** — a `task` whose status is `DONE`, `CLOSED` or `ABANDONED`. Abandonment is step one and deletion is step two: an untouched task is visible as `ABANDONED` for a week and gone after two, and setting its status back revives it within that week. Both steps happen automatically, once per process, before the first write.

In local mode both steps run automatically once per process before the first write. On a server, a background task runs the same abandonment-then-retention sweep hourly across its named graphs. Nothing else accumulates: a truth is a current value with no archive behind it, and relations disappear with the entities they connect.

In local mode the sweep runs on a _write_ rather than at startup, so a read never mutates the graph. On the server, `ASOBI_ABANDON_DAYS` and `ASOBI_RETENTION_DAYS` configure its hourly sweep. In both modes the values resolve from environment first, then `asobi.toml` where applicable, then the default:

| What | Config key | Environment | Default |
| --- | --- | --- | --- |
| Observations kept per entity | `observation_limit` | `ASOBI_OBSERVATION_LIMIT` | 200 |
| Idle days before an open task is abandoned | `abandon_days` | `ASOBI_ABANDON_DAYS` | 7 |
| Days a finished task survives | `retention_days` | `ASOBI_RETENTION_DAYS` | 7 |

Set `abandon_days = 0` to disable abandonment, or `retention_days = 0` to keep finished work indefinitely.

The reason for the second rule is that operational state is relevant for hours, occasionally days. A task that has been `DONE` for a week is not context, it is archaeology — and context is the scarce resource. An earlier design left this to a manual command that was correct in every respect except that it never ran: six weeks of daily use produced a graph that was 96% finished work.

**Preview and purge stale operational state:**

```bash
# Preview only (the default): finished tasks inactive for 30 days
asobi purge

# Narrow the policy to completed tasks older than 90 days
asobi purge --older-than 90

# Apply exactly the previewed policy
asobi purge --older-than 90 --apply
```

The background lifecycle sweep normally handles configured abandonment and retention — see [Lifecycle](#lifecycle). Use `purge` to preview or apply a different age threshold. It only ever considers finished `task` entities; durable knowledge is not something a request can name. A `session` entity created by a pre-0.8 version is now an ordinary durable entity and is not purged. Use `--json` for a machine-readable candidate report. An applied purge also runs `PRAGMA incremental_vacuum`, so the database file shrinks with the graph rather than retaining a free list.

`compact` projects every durable entity to Markdown. `task` entities (epics included) stay graph-only — query them with `search` / `show`.

**Inspect the full graph:**

```bash
asobi stats                                # Quick count of entities, relations, observations
asobi graph | jq '.entities[] | select(.entityType == "task")'
```

### Archival

In local mode the graph is one SQLite file. Copy it to back it up or inspect it:

```bash
cp .asobi/data/asobi.db backup.db          # project-local
cp ~/.local/share/asobi/data/asobi.db .    # XDG
sqlite3 backup.db
```

In remote mode, back up the server's data directory using its deployment's backup procedure; client-local graph paths are not used.

The one thing this genuinely gives up is moving a single entity between two graphs — a project-local one and the XDG one, say. Re-create it with `new`/`truth`/`obs`; it is a handful of commands, and it happens rarely enough that a subgraph traversal engine was the wrong price to pay for it.

**Manage truths (structured key-value attributes):**

```bash
asobi truth "project-x" "language" "rust"
asobi rm-truth "project-x" "language"
```

Writing the same key again replaces the value. Asobi keeps no archive of what it held before: that store was unbounded, had no reader, and where a trail genuinely matters the observations carry it in better form — a task's `status` history said `DISPATCHED` where the observation beside it said "dispatched to codex".

**Install the companion skill.** Asobi ships no `SKILL.md` of its own — this document describes what the CLI _is_, and when to reach for it is agent policy. The maintained skill lives in [harus-skills](https://github.com/azusachino/harus-skills), installed with the [`skills` CLI](https://github.com/vercel-labs/skills):

```bash
npx skills add https://github.com/azusachino/harus-skills --skill asobi --agent universal
```

**Coordinate durable task work:**

```bash
asobi tasks plan "project:epic" --objective "Ship the feature" \
  --task "Implement the change" --task "Verify the result"
asobi tasks list "project:epic"
asobi tasks dispatch                 # select the first READY_TO_DISPATCH task
asobi tasks sync "project:epic:task-1" --note "make check passes" --status DONE
asobi tasks close "project:epic"
```

Use `asobi tasks --help` or `asobi tasks <command> --help` for the complete argument reference. These are graph-backed commands: status is a truth, implementation notes are observations, and child tasks link to their epic via `part_of`.

---

## Command reference

Every command is a single CLI invocation. Local mode needs no server; remote mode uses the configured `asobi-server` and its network-access policy.

`asobi <command> --help` is generated from the same definitions as the binary and is authoritative if this section ever falls behind it.

### Create

```text
asobi new <NAME> <TYPE> [<NAME> <TYPE> ...] [--obs <OBSERVATION> ...]
```

Creates one or more entities from repeated `NAME TYPE` pairs — `new A task B concept` creates two — so the positional count must be a multiple of 2. Names that already exist are silently skipped (`INSERT OR IGNORE`), which makes the command safe to re-run. Repeatable `--obs` seeds observations at creation; with several entities in one call, each seeded observation is added to all of them. Prefer one batched call to many invocations.

```text
asobi obs <NAME> <CONTENT> [<CONTENT> ...]
```

Appends observations to an entity that must already exist. Observations are capped per entity — 200 by default, oldest evicted — configurable through `ASOBI_OBSERVATION_LIMIT` or `observation_limit` in `asobi.toml`.

```text
asobi link <FROM> <TO> <TYPE> [<FROM> <TO> <TYPE> ...]
```

Creates directed relations from repeated `FROM TO TYPE` triples, so the positional count must be a multiple of 3. Upserts on the composite key `(from, to, relation_type)`.

### Read

```text
asobi graph
```

Returns the whole graph as `{ "entities": [...], "relations": [...] }`. Entities carry their truths and observation counts; observation bodies stay lazy.

```text
asobi search [QUERY] [--limit <N>] [--where KEY=VALUE ...]
```

Returns a subgraph of matching entities, in the same payload shape as `graph`, plus the relations between them. Two search paths are merged in order:

1. **FTS5 over observations** — porter stemming with BM25 ranking.
2. **FTS5 over truth values** — so an entity is findable by what its truths say, not only by its observations. This matters for the convention of storing a pitfall's human-readable warning in a `title` truth.
3. **LIKE over entity name and type** — a substring fallback catching exact-name lookups such as `UserPreferences`, and entities with no text at all.

The three are combined with reciprocal rank fusion rather than concatenated, so a strong match in one path competes with a strong match in another and an entity several paths agree on ranks higher. Results come back in that fused order. Each path contributes a fixed candidate pool before fusion, so ranking does not shift when you ask for more results.

FTS5 operators `AND`, `OR`, `NOT` and the `*` prefix wildcard all apply. Bare terms are ANDed, so a multi-word question can match nothing even when every word appears somewhere. Rather than return a silent zero — indistinguishable from "nothing was ever recorded" — `search` retries the query with `OR` and says so on stderr:

```text
WARN no exact match for "deploy without cache bump"; widened to any-term and
     found 10. Narrow with fewer words, or quote an exact phrase.
```

`--where KEY=VALUE` filters the results by entity truths and is repeatable; multiple filters intersect (AND). A query term and `--where` filters likewise intersect. `--limit` defaults to **10** matched nodes — raise it explicitly for a larger ranked read. Use `graph` when the whole graph is genuinely wanted; widening `search` until it returns everything is not the same thing.

```text
asobi show <NAME> [<NAME> ...] [--expand <RELATION_TYPE> ...] [--with-ids]
```

Returns a subgraph for the named entities and the relations among them, eagerly including observations.

- `--expand <RELATION_TYPE>` — repeatable; pulls in entities linked by that relation, e.g. `--expand part_of` to load an epic's tasks.
- `--with-ids` — adds `observationsDetailed`, pairing each observation with its stable integer `id` for use with `update-obs --id` and `rm-obs --id`.
- `--limit <N>` — how many of the most recent observations to return per entity, defaulting to **20**. `--limit 0` returns the whole trail. `observationCount` is always the true total, so a limited read still says how much it left behind. Truths are never limited: there is one row per key and they are the current state.

Fetch heavy content with `show` for the specific entities needed rather than through `graph` or a broad `search`.

### Truths

```text
asobi truth <NAME> <KEY> <VALUE>
asobi rm-truth <NAME> <KEY>
```

`truth` adds or overwrites a key-value fact; `rm-truth` removes one. A truth is the current value and nothing more — writing the same key replaces what was there, with no archive kept.

### Delete

```text
asobi rm <NAME> [<NAME> ...]
asobi update-obs <NAME> <OLD_CONTENT> <NEW_CONTENT> [--id]
asobi rm-obs <NAME> <CONTENT> [--id]
asobi unlink <FROM> <TO> <RELATION_TYPE>
```

`rm` deletes entities and cascades to their observations and relations. `update-obs` atomically replaces one observation; `rm-obs` removes one. Both match on exact content by default, or on the observation ID from `show --with-ids` when given `--id`. `unlink` removes a single relation by its three-part key.

### Workspace

```text
asobi init            # XDG (default) — user-level directories under $HOME
asobi init --local    # project-local — ./.asobi/ plus ./asobi.toml
asobi stats           # entity, relation, and observation counts
asobi capabilities    # the API contract and the selected backend's capabilities
asobi schema [--command NAME]
asobi completions bash|elvish|fish|powershell|zsh
```

Both `init` modes are idempotent. `completions` is generated from the running binary, so the script always matches the installed version.

### Maintenance

```text
asobi compact
asobi purge [--older-than <DAYS>] [--apply]
asobi reset [--force]
```

`compact` projects **durable knowledge** entities — `project`, `concept`, `reference`, `preference`, `standard`, and any legacy `session` entities — into Markdown under `.asobi/topics/`. In 0.8, sessions stopped being a special type. `task` entities are skipped by design; read those with `search`/`show`.

`purge` is a dry run unless given `--apply`, and accepts only `task` entities in a terminal status (`DONE`, `CLOSED`, `ABANDONED`) — durable knowledge is refused. It defaults to entities inactive for 30 days. It never runs implicitly during `graph`, `search`, `compact`, or startup. An applied purge also runs `PRAGMA incremental_vacuum`, so the database file shrinks with the graph rather than retaining a free list.

`reset` deletes every entity, relation, and observation; it prompts unless given `--force`.

### Tasks

```text
asobi tasks plan <EPIC> --objective <TEXT> --task <TITLE>...
asobi tasks list [EPIC] [--all]
asobi tasks dispatch [TASK] [--agent <NAME>]
asobi tasks sync <TASK> [--status <STATUS>] [--note <TEXT>]
asobi tasks close <EPIC> [--lesson <TEXT>]
```

These are ordinary graph entities under a workflow contract: status is a truth, notes are observations, and child tasks link to their epic with `part_of`. Task status moves through `READY_TO_DISPATCH → DISPATCHED → REVIEW → AWAITING_VERIFY → DONE`. `dispatch` claims a task and records the claim atomically — it marks ownership and does **not** launch an agent; omitting `TASK` claims the first ready one. Use `asobi tasks <command> --help` for the full argument list.

Without an `EPIC`, `tasks list` is the "what is open" read: it returns tasks and epics that are not `DONE`, `CLOSED` or `ABANDONED`. Pass `--all` for the complete board including finished work. An entity with no `status` truth counts as open — which is what surfaces an epic whose children are all `DONE` but which was never closed: it appears alone, with no open children under it.

A checkpoint is more useful when it says which revision it was true at, but Asobi does not capture that for you: one graph can serve several repositories — a workspace of submodules resolves to the same graph from every directory — so the commit it would read depends on where the command was run, not on what the task is about. Record it yourself when the handoff warrants it, from the repository the work is actually in:

```bash
asobi truth "[project]:[epic]:task-N" commit "$(git -C path/to/repo rev-parse HEAD)"
```

## Entity types and naming

The type given to `asobi new` determines what `--where` filters, `compact`, and `purge` later see:

| Type         | Use for                                             |
| ------------ | --------------------------------------------------- |
| `project`    | Stable per-project facts and architecture decisions |
| `session`    | Legacy data from pre-0.8 workspaces; ordinary entity in 0.8 |
| `task`       | Epics and their dispatchable child tasks            |
| `concept`    | Decisions, pitfalls, technical definitions          |
| `preference` | Cross-project user or tool preferences              |
| `standard`   | Conventions that apply everywhere                   |
| `reference`  | Pointers to external resources and URLs             |

Only `task` entities stay out of the Markdown projection, and only finished `task` entities are eligible for `purge`; legacy `session` entities are ordinary durable knowledge in 0.8.

Names are hierarchical and colon-separated — `project-x`, `project-x:task:deploy`, `project-x:epic`, `project-x:epic:task-1` — and preserve case and dots, so `CLAUDE.md` and `UserPreferences` are valid names. Relations read as verb phrases: `part_of`, `depends_on`, `supersedes`, `extends`, `uses`, `blocks`.

## Response contract

### Streams and exit codes

**Mutating** commands print a one-line confirmation (`Entity 'X' created.`, `Observation added.`) to **stderr** and leave **stdout empty** on success. A scripted caller must branch on the exit code, not on stdout being non-empty.

**Read** commands (`graph`, `search`, `show`, `stats`, `capabilities`, `schema`) write their JSON payload to **stdout**.

The global `--json` flag makes a mutation also print the affected entities, and the relations among them, to stdout — `asobi new A task --json` removes the follow-up `show` round-trip, and `rm --json` returns `{ "deleted": [...] }`. It has no effect on read commands, which already emit JSON.

### Machine-readable response contract

Commands retain their existing JSON payload shapes. `asobi schema` is the compatibility promise and discovery surface:

```bash
asobi schema
asobi schema --command show
```

The schema document carries its own `schemaVersion`. Use the command-specific schema to validate and parse the corresponding payload; no extra response wrapper is required — a read writes its payload itself, not an envelope around it.

### Output format

Asobi operates under a **lazy-read contract** to minimize token overhead.

The payload for `graph` and `search` is a lazy JSON structure (excluding `observations`, providing only `truths` and `observationCount`):

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "truths": {
        "key": "value"
      },
      "observationCount": 12
    }
  ],
  "relations": [
    {
      "from": "string",
      "to": "string",
      "relationType": "string"
    }
  ]
}
```

`show` eagerly returns all `observations`:

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "observations": ["string", ...],
      "truths": {
        "key": "value"
      },
      "observationCount": 12,
      "observationsDetailed": [
        {
          "id": 123,
          "content": "string"
        }
      ]
    }
  ],
  "relations": [
    {
      "from": "string",
      "to": "string",
      "relationType": "string"
    }
  ]
}
```

`observationsDetailed` is present only when `--with-ids` was passed.

### Search behavior

`search` uses SQLite FTS5 with porter stemming and BM25 ranking, followed by a name/type substring fallback. Practical implications:

- `search "run"` → matches the indexed term "run" (use the exact term when needed)
- `search "implement"` → matches the indexed term "implement"
- `search "tokio async"` → finds entities with both words (ranked higher) or either word
- `search "UserPreferences"` → exact name match via LIKE fallback (entity has no observations)
- `search "AND AND"` → invalid full-text syntax, silently falls back to LIKE, returns empty
- `search "auth" --limit 500` → return more than the default 10 matched nodes
- `search --where KEY=VALUE` → filters matching entities by truth values (e.g. `--where status=READY`). Can be repeated; multiple filters perform an intersection (AND condition). If a query term is also provided, it matches the intersection of the filters and the FTS/LIKE results.

For exact entity retrieval, prefer `show` over `search`:

```bash
asobi show "project-x:task:deploy" "UserPreferences"
```

## Remote workspaces

A workspace can use a whole named graph on an Asobi server. Add `remote` to its `asobi.toml`; `graph` selects the server graph and defaults to `asobi`:

```toml
remote = "https://asobi.h.azusachino.com"
graph = "workstation"
```

`ASOBI_REMOTE` and `ASOBI_GRAPH` override those keys. When `remote` is set, all graph and task calls go to that server; local `data_dir` and `ASOBI_DATABASE_URL` are not used. The `observation_limit` and `topics_dir` stay client-side, so `compact` writes Markdown locally.

The API version is encoded in the `/v3` URL; there is no `server.hello` call or separate handshake. Each operation is plain JSON over `POST /v3/graphs/<graph>/<operation>`; successful calls return JSON, and errors use a non-2xx status with `{ "kind", "message" }`. `GET /healthz` returns 200 without opening a graph and is for liveness probes only. The first remote operation is the reachability probe (a write command first makes a read-only `maintenance.location` call). If that call cannot connect, times out after about two seconds, or receives gateway HTTP 502/503/504, the process warns on stderr and uses the local graph for the whole command; outage writes stay local and are never merged later. Once any remote call succeeds, a later failure is an error and never switches backend mid-command. A non-protocol response fails with `server does not speak API v3`. The process reuses one HTTP client/connection. Use a build with the `remote` feature (`cargo install asobi --features remote`); a local-only build fails clearly if it finds `remote` configured.

## The graph server: `asobi-server`

One long-lived `asobi-server` process holds **named graphs** — one SQLite file per graph in its required data directory — and serves them over HTTP to workspaces configured with `remote` (see ADR 0005). Build/install `asobi-server` separately; the remote client is optional on the `asobi` CLI.

```bash
asobi-server --listen 127.0.0.1:8300 --data-dir /srv/asobi-data
```

- **Both arguments are required.** `--data-dir` is the server's own data directory — created if missing and never resolved from `asobi.toml`/XDG, so a server and a local CLI on one host cannot silently share a graph file, and the server's sweep never walks the CLI's directory. Each graph is `<data-dir>/<name>.db`. Graph names match `^[a-z0-9-]+$` — they become file names, so anything else is refused with `422 invalid` and no file is created. Naming a graph that does not exist yet creates it on first use.
- **Route:** `POST /v3/graphs/<graph>/<operation>` with the operation's request object as the body; a success is `200` with the result JSON. A failure is a non-2xx status with `{"kind", "message"}`. Any other verb on a known path is `405 badRequest`; any other path is `404`.
- **Requests are concurrent** (one sqlx pool per graph); SQLite serialises writes through WAL and the busy timeout, and task claims and abandonment run in `BEGIN IMMEDIATE` transactions.
- **Sweeps run in the background** on a one-hour interval over every graph: idle open tasks are abandoned before retention deletes finished tasks, exactly as in local mode, using the server's own `retention_days` / `abandon_days` configuration.
- **`maintenance.reset` is refused over the network** (`501 unsupported`): run `asobi reset` on the server host against the file directly when that is intended.
- **Liveness:** `GET /healthz` returns 200 without opening a graph; use it for container and cluster probes.
- **Access:** no authentication — the server must only be reachable on your tailnet; binding it to a public interface is out of contract.

## Running in Sandboxed Environments (Codex, etc.)

When running in sandboxed or highly restricted environments (such as Codex, Nix build sandboxes, or certain containerized runners), the environment might impose constraints on directory write access or shared-memory creation. Asobi can be configured to run smoothly in these environments using the following techniques:

### Project-Local Workspace

Use the project-local setup to avoid writing to the global `~/.local` (XDG) directory, which may be read-only or non-existent:

```bash
asobi init --local
```

This writes an `asobi.toml` file in the current working directory and places database and configurations within the `./.asobi/` subdirectory.

### Custom Database Paths

You can override Asobi's home or database locations using environment variables:

- **`ASOBI_HOME`**: Changes the base directory under which Asobi looks for configuration, data, and topics (e.g. `ASOBI_HOME=/tmp/asobi`).
- **`ASOBI_DATABASE_URL`**: Specifies the direct path to the database file itself (e.g. `ASOBI_DATABASE_URL=/tmp/asobi-custom.db`).

### SQLite concurrency

SQLite is opened in WAL mode with foreign keys enabled and a bounded `ASOBI_BUSY_TIMEOUT` (15 seconds by default). Writes use immediate transactions, so multiple agents can read concurrently while writes serialize safely. Task dispatch claims the task and records the claim observation atomically.
