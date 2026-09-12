use clap::{Parser, Subcommand, ValueEnum};

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
        /// Most recent observations to return per entity; 0 for the whole trail.
        /// `observationCount` always reports the true total.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Sync durable knowledge entities to Markdown topics
    Compact {},
    /// Preview or delete finished sessions and tasks.
    ///
    /// This runs automatically once per process before the first write, so it
    /// is normally only reached to preview what would go, or to sweep a
    /// narrower window than the configured one.
    Purge {
        /// Only consider entities finished for at least this many days
        #[arg(long, default_value_t = crate::storage::DEFAULT_RETENTION_DAYS)]
        older_than: u32,
        /// Apply the deletion; without this flag purge is a preview
        #[arg(long)]
        apply: bool,
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
    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: CompletionShell,
    },

    /// Reset the knowledge graph (delete all entities, relations, and observations)
    Reset {
        /// Force reset without confirmation
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

#[derive(Clone, Debug, ValueEnum)]
pub(crate) enum CompletionShell {
    Bash,
    Elvish,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
    Zsh,
}

impl From<CompletionShell> for clap_complete::Shell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => Self::Bash,
            CompletionShell::Elvish => Self::Elvish,
            CompletionShell::Fish => Self::Fish,
            CompletionShell::PowerShell => Self::PowerShell,
            CompletionShell::Zsh => Self::Zsh,
        }
    }
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
        /// Install skills by source-relative directory path, unique suffix, or name
        #[arg(long, num_args = 1..)]
        select: Option<Vec<String>>,
        /// Only walk this subdirectory of the checkout (e.g. a source that
        /// mirrors skills across several tool-specific directories)
        #[arg(long)]
        subdir: Option<std::path::PathBuf>,
        /// Pin to a commit, tag, or branch instead of the default branch
        #[arg(long)]
        rev: Option<String>,
    },
    /// Reconcile installed skills with the `[skills]` block in `asobi.toml`
    Sync,
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
