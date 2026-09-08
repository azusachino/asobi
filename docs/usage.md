# Asobi: Usage Guide

This is Asobi's interface reference: what each command does, what it accepts, and what it returns. It describes the CLI and nothing more.

It deliberately does not prescribe a session workflow — when to read the graph, what to write at closeout, how to sequence a task board. That guidance is agent policy rather than a property of the tool, it differs between users, and keeping a copy here produced a set of documents that drifted into contradicting each other. Workflow lives in the [`asobi` skill](https://github.com/azusachino/harus-skills/blob/main/skills/asobi/SKILL.md), which cites this document for exact contracts.

## For humans

### Installation

From source via cargo (Rust 1.85+ toolchain required for edition 2024):

```bash
cargo install --git https://github.com/azusachino/asobi asobi
```

Prebuilt binary via `cargo-binstall` (once GitHub releases are published):

```bash
cargo binstall asobi
```

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

An optional `[skills]` block in the same file declares the skill set that `asobi skills sync` reconciles — see [Declare skills in `asobi.toml`](#common-workflows).

Path resolution order at runtime: project-local `asobi.toml` → project-local `.asobi/` → XDG. Both `init` modes are idempotent.

Add `.asobi/` to `.gitignore`; the `asobi.toml` itself can be checked in.

### Common workflows

**Recall stored state by truth or by name:**

```bash
asobi search --where status=IN_PROGRESS
asobi show "my-project:session"
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

Use `graph` for full export. `search` is intentionally top-K by default so a broad term does not accidentally return the whole graph.

**Persist state — truths for the current value, observations for the trail:**

```bash
asobi truth "my-project:session" "status" "DONE"
asobi truth "my-project:session" "next" "implement FTS5 index"
asobi obs "my-project:session" "completed 2026-05-21: added the FTS5 index"
```

A truth is the right home for anything read back as _current_ state, because writing the same key updates it in place. Observations accumulate and are evicted at the cap, so a next-action stored as an observation can silently age out.

### Lifecycle

Two rules, and no others:

1. **Durable entities live forever** — `project`, `concept`, `reference`, `preference`, `standard`. Each keeps its most recent 200 observations; older ones are evicted as new ones arrive.
2. **Finished operational entities are deleted after 7 days** — a `session` or `task` whose status is `DONE`, `CLOSED` or `ABANDONED`. This happens automatically, once per process, before the first write.

That is the whole of it. Nothing else accumulates: a truth is a current value with no archive behind it, and relations disappear with the entities they connect.

The sweep runs on a _write_ rather than at startup, so a read never mutates the graph. Both numbers are configurable, resolved the same way — environment variable first, then `asobi.toml`, then the default:

| What | Config key | Environment | Default |
| --- | --- | --- | --- |
| Observations kept per entity | `observation_limit` | `ASOBI_OBSERVATION_LIMIT` | 200 |
| Days a finished task survives | `retention_days` | `ASOBI_RETENTION_DAYS` | 7 |

Set `retention_days = 0` to disable the sweep and keep finished work indefinitely.

The reason for the second rule is that operational state is relevant for hours, occasionally days. A task that has been `DONE` for a week is not context, it is archaeology — and context is the scarce resource. An earlier design left this to a manual command that was correct in every respect except that it never ran: six weeks of daily use produced a graph that was 96% finished work.

**Preview and purge stale operational state:**

```bash
# Preview only (the default): terminal sessions/tasks inactive for 30 days
asobi purge

# Narrow the policy to completed tasks older than 90 days
asobi purge --older-than 30

# Apply exactly the previewed policy
asobi purge --older-than 30 --apply
```

This normally runs by itself — see [Lifecycle](#lifecycle). Reach for it to preview what would go, or to sweep a narrower window than the configured one. It only ever considers finished `session` and `task` entities; durable knowledge is not something a request can name. Use `--json` for a machine-readable candidate report. An applied purge also runs `PRAGMA incremental_vacuum`, so the database file shrinks with the graph rather than retaining a free list.

`compact` syncs only durable _knowledge_ entities (project, decisions, references, preferences) to Markdown. Volatile state (`session`, `task`) stays graph-only — query it with `search` / `show`, and use `export` / `backup` for full archival. Skills are not in the graph at all; they live on disk under the skills directory.

**Inspect the full graph:**

```bash
asobi stats                                # Quick count of entities, relations, observations
asobi graph | jq '.entities[] | select(.entityType == "session")'
```

### Backup, restore, and portable export

| Goal | Command | Includes |
| --- | --- | --- |
| Portable handoff | `asobi export -o graph.json` | Entities, observations, truths, relations |
| Scoped handoff | `asobi export --scope "proj:epic" -o epic.json` | One epic subtree |
| Full SQLite archive | `asobi backup` | Complete database, including task state. Skills live on disk and are backed up with the repository, not here. |

```bash
asobi import graph.json
asobi backup                       # backups/asobi-<timestamp>.db; keep newest 3
asobi backup --keep 5
asobi backup -o /secure/asobi.db   # explicit path; never overwrites
asobi restore /secure/asobi.db     # validate, save current DB, then prompt
asobi restore /secure/asobi.db --force
```

- `--keep` applies only to managed snapshots, not an explicit `-o` path.
- Snapshots are integrity-checked and owner-only on Unix.
- Restore writes `backups/pre-restore-*.db`, closes live handles, atomically replaces the database, and removes stale WAL sidecars.

Scoped export is designed for handing an epic to another agent:

- Includes each root, transitive `part_of` children, and one-hop `depends_on` targets.
- `--rationale` adds one hop of `supersedes`/`extends` from cited decisions.
- Excludes `session`, `preference`, and `standard` entities.
- Produces ordinary JSON consumed by `asobi import`.

```bash
asobi export --scope "proj:epic" --scope "proj:other-epic" -o bundle.json
asobi export --scope "proj:epic" --rationale -o bundle.json
```

**Manage truths (structured key-value attributes):**

```bash
asobi truth "project-x" "language" "rust"
asobi rm-truth "project-x" "language"
```

Writing the same key again replaces the value. Asobi keeps no archive of what it held before: that store was unbounded, had no reader, and where a trail genuinely matters the observations carry it in better form — a task's `status` history said `DISPATCHED` where the observation beside it said "dispatched to codex".

**Manage skills (reusable workflows and knowledge):**

```bash
asobi skills install https://github.com/azusachino/asobi-skills --all
asobi skills
asobi skills show my-skill
asobi skills update
asobi skills remove asobi-skills
```

**Declare skills in `asobi.toml` and reconcile them:**

```toml
[skills]
path = ".agents/skills"          # optional; this is the default

[[skills.source]]
url = "https://github.com/azusachino/asobi-skills"
select = ["writing-plans", "code-review"]

[[skills.source]]
url = "https://github.com/some-org/multi-tool-skills"
select = ["some-skill"]
subdir = "skills"                # only walk this directory of the checkout
rev = "v1.4.0"                   # pin to a commit, tag, or branch
```

```bash
asobi skills sync
```

`sync` treats the config as the whole truth: it installs what is declared, prunes what is not, and writes each selected skill to `<path>/<source-slug>@<skill-name>/SKILL.md`. Directories without `@` in the name — vendored checkouts, hand-written skills — are left alone. Declare exactly one of `all = true` or `select = [...]` per source.

The skills directory is the store of record: since 0.7 a skill exists on disk and nowhere else, so it does not appear in `graph`, `search`, or `show`, and `rg` over the skills directory is how you search one. `path` defaults to `.agents/skills`, resolved against the `asobi.toml` that declares it, or against the discovered workspace root when no config declares a `[skills]` block — so `skills` and `skills show` work under a plain `asobi init` too.

Alongside the skill directories, `sync` writes `.asobi-skills.json` recording each skill's source and the exact commit it came from. `asobi skills` reports that commit. Committing the whole tree, manifest included, is what turns an upstream skill change into a reviewable diff.

`rev` completes that loop. Without it a re-sync silently adopts whatever the source has moved to since; with it, adopting a new revision is an edit someone makes on purpose. An annotated tag resolves to the commit it points at, not the tag object, so the recorded version is always a commit.

One deliberate divergence from the [Agent Skills specification](https://agentskills.io/specification): it requires a skill's directory name to equal its frontmatter `name`, which assumes a skill is authored in place. Asobi installs many sources into one tree, so it names directories `<source-slug>@<skill-name>` — two sources may ship the same skill name, and agent hosts surface the directory name as the skill's identity. Everything else the spec says about a skill is enforced by `make check`, which runs the reference validator over each installed skill.

Some sources mirror every skill across several tool-specific directories (`.opencode/`, `.kiro/`, a canonical `skills/`, ...) with the same `name:` in each copy — that collides on install, since a skill name must be unique within a source. `subdir` scopes the walk to one directory of the checkout so the mirrors are never seen; `asobi skills install <url> --subdir <path> ...` does the same for the imperative form.

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

Every command is a single CLI invocation. No server to start, no authentication; graph operations complete in under 10ms.

`asobi <command> --help` is generated from the same definitions as the binary and is authoritative if this section ever falls behind it.

### Create

```
asobi new <NAME> <TYPE> [<NAME> <TYPE> ...] [--obs <OBSERVATION> ...]
```

Creates one or more entities from repeated `NAME TYPE` pairs — `new A task B concept` creates two — so the positional count must be a multiple of 2. Names that already exist are silently skipped (`INSERT OR IGNORE`), which makes the command safe to re-run. Repeatable `--obs` seeds observations at creation; with several entities in one call, each seeded observation is added to all of them. Prefer one batched call to many invocations.

```
asobi obs <NAME> <CONTENT> [<CONTENT> ...]
```

Appends observations to an entity that must already exist. Observations are capped per entity — 200 by default, oldest evicted — configurable through `ASOBI_OBSERVATION_LIMIT` or `observation_limit` in `asobi.toml`.

```
asobi link <FROM> <TO> <TYPE> [<FROM> <TO> <TYPE> ...]
```

Creates directed relations from repeated `FROM TO TYPE` triples, so the positional count must be a multiple of 3. Upserts on the composite key `(from, to, relation_type)`.

### Read

```
asobi graph
```

Returns the whole graph as `{ "entities": [...], "relations": [...] }`. Entities carry their truths and observation counts; observation bodies stay lazy.

```
asobi search [QUERY] [--limit <N>] [--where KEY=VALUE ...]
```

Returns a subgraph of matching entities, in the same payload shape as `graph`, plus the relations between them. Two search paths are merged in order:

1. **FTS5 over observations** — porter stemming with BM25 ranking. `"tokio async"` ranks entities containing both words higher, and the FTS5 operators `AND`, `OR`, `NOT` and the `*` prefix wildcard all apply.
2. **LIKE over entity name and type** — a substring fallback that always runs, catching exact-name lookups such as `UserPreferences` and entities that have no observations at all.

`--where KEY=VALUE` filters the results by entity truths and is repeatable; multiple filters intersect (AND). A query term and `--where` filters likewise intersect. `--limit` defaults to **10** matched nodes — raise it explicitly for a larger ranked read. Use `graph` when the whole graph is genuinely wanted; a deliberately broad `search` query is not an export.

```
asobi show <NAME> [<NAME> ...] [--expand <RELATION_TYPE> ...] [--with-ids]
```

Returns a subgraph for the named entities and the relations among them, eagerly including observations and skill bodies.

- `--expand <RELATION_TYPE>` — repeatable; pulls in entities linked by that relation, e.g. `--expand part_of` to load an epic's tasks.
- `--with-ids` — adds `observationsDetailed`, pairing each observation with its stable integer `id` for use with `update-obs --id` and `rm-obs --id`.
- `--limit <N>` — how many of the most recent observations to return per entity, defaulting to **20**. `--limit 0` returns the whole trail. `observationCount` is always the true total, so a limited read still says how much it left behind. Truths are never limited: there is one row per key and they are the current state.

Fetch heavy content with `show` for the specific entities needed rather than through `graph` or a broad `search`.

### Truths

```
asobi truth <NAME> <KEY> <VALUE>
asobi rm-truth <NAME> <KEY>
```

`truth` adds or overwrites a key-value fact; `rm-truth` removes one. A truth is the current value and nothing more — writing the same key replaces what was there, with no archive kept.

### Delete

```
asobi rm <NAME> [<NAME> ...]
asobi update-obs <NAME> <OLD_CONTENT> <NEW_CONTENT> [--id]
asobi rm-obs <NAME> <CONTENT> [--id]
asobi unlink <FROM> <TO> <RELATION_TYPE>
```

`rm` deletes entities and cascades to their observations and relations. `update-obs` atomically replaces one observation; `rm-obs` removes one. Both match on exact content by default, or on the observation ID from `show --with-ids` when given `--id`. `unlink` removes a single relation by its three-part key.

### Workspace

```
asobi init            # XDG (default) — user-level directories under $HOME
asobi init --local    # project-local — ./.asobi/ plus ./asobi.toml
asobi stats           # entity, relation, and observation counts
asobi capabilities    # the API contract and the selected backend's capabilities
asobi schema [--command NAME]
asobi completions bash|elvish|fish|powershell|zsh
```

Both `init` modes are idempotent. `completions` is generated from the running binary, so the script always matches the installed version.

### Maintenance

```
asobi compact
asobi purge [--older-than <DAYS>] [--apply]
asobi reset [--force]
```

`compact` projects **durable knowledge** entities — `project`, `concept`, `reference`, `preference`, `standard` — and their truths into Markdown under `.asobi/topics/`. Volatile `session` and `task` entities and self-indexing `skill` entities are skipped by design; read those with `search`/`show` and archive them with `export` or `backup`.

`purge` is a dry run unless given `--apply`, and accepts only `session` entities plus terminal task statuses (`DONE`, `CLOSED`, `ABANDONED`) — durable knowledge is refused, and skills are not in the graph to begin with. It defaults to entities inactive for 30 days. It never runs implicitly during `graph`, `search`, `compact`, or startup. An applied purge also runs `PRAGMA incremental_vacuum`, so the database file shrinks with the graph rather than retaining a free list.

`reset` deletes every entity, relation, and observation; it prompts unless given `--force`.

### Skills

```
asobi skills                                                    # list, grouped by source
asobi skills install <SOURCE> [--all | --select <NAME>...] [--subdir <PATH>] [--rev <REV>]
asobi skills sync
asobi skills update [SOURCE]
asobi skills remove <NAME | SOURCE>
asobi skills show <NAME>
```

`install` takes a git URL or a local path; git sources are shallow-cloned into a reused cache under `.asobi/caches/<slug>`. Frontmatter supplies the metadata, with the name falling back to the file or directory name. `--all` is a full sync of that source, pruning skills deleted or renamed upstream; `--select` and the interactive picker are additive. Passing neither flag opens a numbered picker, which needs a TTY and otherwise errors asking for a flag. `--subdir` scopes the walk to one directory of the checkout, for sources that mirror the same skills across several tool-specific directories and would otherwise collide on name. `--rev` pins to a commit, tag, or branch instead of the default branch. Installing one source never disturbs another's skills.

A skill is a directory containing `SKILL.md`; a loose `<name>.md` file is not a skill and is not installed. Its Markdown comes across with it — `references/*.md` and any sibling `.md` — so the on-demand references the spec relies on still resolve after install.

Nothing else is copied. `scripts/`, `assets/` and tool-specific config are fetched from a git URL, and that is where published attack research finds payloads hidden, since scanners read the body and not the artifacts beside it. Skipped files are named in a warning; fetch one deliberately from the source if a step genuinely needs it.

`sync` reconciles against the `[skills]` block in the discovered `asobi.toml`, as described under [Common workflows](#common-workflows). `update` refreshes from cache via `git fetch` and `reset --hard`, re-cloning if that fails; it needs `git` on `PATH`, and a scoped `update <source>` leaves other sources alone. `show` prints a skill's `SKILL.md` as raw Markdown, matched on its frontmatter name or its directory name. Never hand-edit an installed skill — the next sync overwrites it; edit the source repository instead.

### Tasks

```
asobi tasks plan <EPIC> --objective <TEXT> --task <TITLE>...
asobi tasks list [EPIC] [--all]
asobi tasks dispatch [TASK] [--agent <NAME>]
asobi tasks sync <TASK> [--status <STATUS>] [--note <TEXT>]
asobi tasks close <EPIC> [--lesson <TEXT>]
```

These are ordinary graph entities under a workflow contract: status is a truth, notes are observations, and child tasks link to their epic with `part_of`. Task status moves through `READY_TO_DISPATCH → DISPATCHED → REVIEW → AWAITING_VERIFY → DONE`. `dispatch` claims a task and records the claim atomically — it marks ownership and does **not** launch an agent; omitting `TASK` claims the first ready one. Use `asobi tasks <command> --help` for the full argument list.

Without an `EPIC`, `tasks list` is the "what is open" read: it returns tasks and epics that are not `DONE`, `CLOSED` or `ABANDONED`. Pass `--all` for the complete board including finished work. An entity with no `status` truth counts as open — which is what surfaces an epic whose children are all `DONE` but which was never closed: it appears alone, with no open children under it.

`tasks sync` and `tasks close` also record a `commit` and `branch` truth when run inside a git worktree, so a checkpoint says which revision it was true at. A detached `HEAD` records the commit and no branch, and running outside a repository records neither — neither case is an error.

## Entity types and naming

The type given to `asobi new` determines what `--where` filters, `compact`, and `purge` later see:

| Type         | Use for                                             |
| ------------ | --------------------------------------------------- |
| `project`    | Stable per-project facts and architecture decisions |
| `session`    | Volatile session state                              |
| `task`       | Epics and their dispatchable child tasks            |
| `concept`    | Decisions, pitfalls, technical definitions          |
| `preference` | Cross-project user or tool preferences              |
| `standard`   | Conventions that apply everywhere                   |
| `reference`  | Pointers to external resources and URLs             |

Only the durable types reach Markdown through `compact`, and only `session` and terminal `task` entities are eligible for `purge`, so a decision typed as `session` is both invisible to topics and reachable by retention.

Names are hierarchical and colon-separated — `project-x`, `project-x:session`, `project-x:epic`, `project-x:epic:task-1` — and preserve case and dots, so `CLAUDE.md` and `UserPreferences` are valid names. Installed skills are named `skill:<source-slug>:<name>`. Relations read as verb phrases: `part_of`, `depends_on`, `supersedes`, `extends`, `uses`, `blocks`.

## Response contract

### Streams and exit codes

**Mutating** commands print a one-line confirmation (`Entity 'X' created.`, `Observation added.`) to **stderr** and leave **stdout empty** on success. A scripted caller must branch on the exit code, not on stdout being non-empty.

**Read** commands (`graph`, `search`, `show`, `stats`, `export`, `capabilities`, `schema`) write their JSON payload to **stdout**. `asobi skills show` writes raw Markdown instead, since its purpose is to be read.

The global `--json` flag makes a mutation also print the affected entities, and the relations among them, to stdout — `asobi new A task --json` removes the follow-up `show` round-trip, and `rm --json` returns `{ "deleted": [...] }`. It has no effect on read commands, which already emit JSON.

### Machine-readable response contract

Commands retain their existing JSON payload shapes. `asobi schema` is the compatibility promise and discovery surface:

```bash
asobi schema
asobi schema --command show
```

The schema document carries its own `schemaVersion`, independent from the storage/export `apiVersion`. Use the command-specific schema to validate and parse the corresponding payload; no extra response wrapper is required.

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
asobi show "project-x:session" "UserPreferences"
```

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
