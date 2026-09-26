# Asobi

A persistent knowledge graph that agents and people read and write through one CLI, either on the local device or shared through a server.

## Language

### Where the graph lives

**Graph**:
One self-contained set of entities and relations; nothing relates across two graphs.
_Avoid_: database, store, memory

**Workspace**:
A directory tree governed by one `asobi.toml`, which decides whether its graph is local or on a server.
_Avoid_: project, repo

**Server**:
The one long-running Asobi process that holds named graphs for every device to share.
_Avoid_: daemon, backend, host

**Graph name**:
The name that selects one graph on a server; a remote workspace uses exactly one, `asobi` unless it names another, and naming a graph the server does not hold yet creates it.
_Avoid_: namespace, database name

**Local mode**:
A workspace whose graph is only on this device.

**Remote mode**:
A workspace whose whole graph is on a server; there is no partly-remote workspace.
_Avoid_: shared mode, sync mode

### What the graph holds

**Task**:
A unit of work with a status; open tasks are the whole record of where work stands and what comes next.
_Avoid_: session, checkpoint, todo

**Epic**:
A task that groups child tasks planned together toward one objective.
_Avoid_: project, milestone

**Abandoned**:
The terminal status a task receives automatically once it has had no activity for too long.
_Avoid_: stale, expired
