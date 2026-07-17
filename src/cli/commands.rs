use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "asobi")]
#[command(version)]
#[command(about = "Asobi: Knowledge Graph & Memory CLI", long_about = None)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
    /// After a mutation, print the affected entity/entities as JSON to stdout
    /// (instead of leaving stdout empty). No effect on read commands, which
    /// already emit JSON.
    #[arg(long, global = true)]
    pub(crate) json: bool,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Ingest a file or directory into the document tier
    #[cfg(feature = "documents")]
    Ingest {
        /// Path to file or directory
        path: String,
    },
    /// Query topics or chunks using hybrid semantic + keyword search
    #[cfg(feature = "documents")]
    Query {
        /// Query string
        query: String,
        /// Maximum number of matched topics to return
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Print results as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create new entities in the knowledge graph.
    ///
    /// Accepts one or more `NAME TYPE` pairs:
    /// `new A task B concept` creates two entities.
    New {
        /// Flat list of `NAME TYPE` pairs (count must be a multiple of 2)
        #[arg(num_args = 2.., value_names = ["NAME", "TYPE"])]
        pairs: Vec<String>,
        /// Seed observations at creation: `--obs VALUE` (repeatable)
        #[arg(long = "obs", value_name = "OBSERVATION")]
        observations: Vec<String>,
    },
    /// Create relations between entities.
    ///
    /// Accepts one or more `FROM TO TYPE` triples:
    /// `link A B uses C D blocks` creates two relations.
    Link {
        /// Flat list of `FROM TO TYPE` triples (count must be a multiple of 3)
        #[arg(num_args = 3.., value_names = ["FROM", "TO", "TYPE"])]
        triples: Vec<String>,
    },
    /// Add observations to existing entities
    Obs {
        name: String,
        #[arg(num_args = 1..)]
        contents: Vec<String>,
    },
    /// Add or update a truth for an entity
    Truth {
        name: String,
        key: String,
        value: String,
    },
    /// Delete a specific truth for an entity
    RmTruth { name: String, key: String },
    /// Show an entity's truth change history (superseded values with validity windows)
    History {
        name: String,
        /// Limit to a single truth key
        key: Option<String>,
    },
    /// Delete entities and their relations
    Rm { names: Vec<String> },
    /// Delete specific observations
    RmObs {
        name: String,
        content: String,
        /// Match by observation ID instead of content
        #[arg(long)]
        id: bool,
    },
    /// Update an existing observation atomically (replaces old content with new content)
    UpdateObs {
        name: String,
        old_content: String,
        new_content: String,
        /// Match by observation ID instead of content
        #[arg(long)]
        id: bool,
    },
    /// Delete specific relations
    Unlink {
        from: String,
        to: String,
        relation_type: String,
    },
    /// Read the entire knowledge graph
    Graph,
    /// Search for nodes
    Search {
        /// Search query terms
        query: Option<String>,
        /// Maximum number of matched nodes to return
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Filter by entity truths: `--where KEY=VALUE` (repeatable)
        #[arg(long = "where", value_name = "KEY=VALUE")]
        filters: Vec<String>,
    },
    /// Retrieve specific nodes by name
    Show {
        names: Vec<String>,
        /// Expand relations of specified type(s) to include linked entities
        #[arg(long, value_name = "RELATION_TYPE")]
        expand: Vec<String>,
        /// Include observation IDs in detailed list
        #[arg(long)]
        with_ids: bool,
    },
    /// Report near-duplicate topics, prune sessions, and sync Graph to MD
    #[cfg(feature = "documents")]
    Compact {
        /// Prune sessions older than N days
        #[arg(long, default_value = "90")]
        older_than: u32,
    },
    /// Initialise a Asobi workspace (XDG by default, `--local` for cwd)
    Init {
        /// Create `.asobi/` and `asobi.toml` in the current directory
        /// instead of the user-level XDG paths.
        #[arg(long)]
        local: bool,
    },
    /// Show statistics about the knowledge graph
    Stats {
        /// Show observation counts and limits per entity
        #[arg(long)]
        per_entity: bool,
    },
    /// Report the API contract and selected backend capabilities
    Capabilities,
    /// Emit JSON Schema for command payloads
    Schema {
        /// Restrict output to one command's response schema
        #[arg(long)]
        command: Option<String>,
    },

    /// Export the knowledge graph to a JSON file
    Export {
        /// Path to the output JSON file
        #[arg(short, long)]
        output: Option<String>,
        /// Restrict the export to the subgraph rooted at these entities
        /// (repeatable). Pulls each root, its `part_of` children (transitively),
        /// and the `depends_on` targets they cite. Omit to export the whole graph.
        #[arg(long)]
        scope: Vec<String>,
        /// With --scope, also follow one hop of `supersedes`/`extends` off the
        /// cited leaves (the rationale chain behind a decision).
        #[arg(long)]
        rationale: bool,
    },
    /// Import a knowledge graph from a JSON file
    Import {
        /// Path to the input JSON file
        file: String,
    },
    /// Reset the knowledge graph (delete all entities, relations, and observations)
    Reset {
        /// Force reset without confirmation
        #[arg(long)]
        force: bool,
    },
    /// Snapshot the database to a single consistent file (VACUUM INTO)
    Backup {
        /// Destination path (default: `<data_dir>/backups/asobi-<timestamp>.db`)
        #[arg(short, long)]
        output: Option<String>,
        /// Snapshots to retain in the default backup directory (oldest pruned)
        #[arg(long, default_value_t = 3)]
        keep: usize,
    },
    /// Replace the live database with a snapshot file
    Restore {
        /// Path to the snapshot file to restore from
        file: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Manage, install, and update AI agent skills
    Skills {
        #[command(subcommand)]
        subcommand: Option<SkillsCommands>,
    },
    /// Plan and coordinate durable task-dispatcher work
    Tasks {
        #[command(subcommand)]
        subcommand: Option<crate::tasks::TasksCommands>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum SkillsCommands {
    /// Install skills from a git repository or local path
    Install {
        /// Git URL or local directory path
        source: String,
        /// Install all skills found
        #[arg(long)]
        all: bool,
        /// Install specific skills by name
        #[arg(long, num_args = 1..)]
        select: Option<Vec<String>>,
    },
    /// Update installed skills from their sources
    Update {
        /// Specific source URL or slug to update (updates all if omitted)
        source: Option<String>,
    },
    /// Remove an installed skill or all skills from a source
    Remove {
        /// Name of the skill (e.g. skill:slug:name) or source slug/URL
        target: String,
    },
    /// Show the raw body of an installed skill (useful for humans to read without JSON escaping)
    Show {
        /// Name of the skill (fully qualified e.g. skill:slug:name, or short name)
        name: String,
    },
}
