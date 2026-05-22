# Rosemary Memory Improvement Plan

This plan addresses data integrity, normalization, and query capability improvements to enhance Rosemary's memory management.

## Objective
Reduce memory fragmentation, improve search accuracy, and provide richer context to agents by standardizing data ingestion and augmenting search capabilities.

## Key Files & Context
- `src/mcp.rs`: Request handling, parameter parsing, and ingestion interface.
- `src/db.rs`: Database operations and SQL schema.
- `src/embed/`: Existing vector search infrastructure.

## Phase 1: Data Normalization & Integrity
*   **Task 1.1: Canonical Key Normalization**: Implement a normalization layer in `src/mcp.rs` to enforce consistent entity names (lowercase, kebab-case) before DB ingestion.
*   **Task 1.2: Input Validation**: Add validation logic to `EntityInput` and related structs in `mcp.rs` to reject malformed data early.

## Phase 2: Enhanced Query Capabilities
*   **Task 2.1: Multi-Modal Search (Keyword + Vector)**: Integrate vector similarity search (using `src/embed/`) into `mcp_search_nodes` in `db.rs` to augment FTS5 results.
*   **Task 2.2: Context-Aware Retrieval (1-Hop Expansion)**: Modify `mcp_search_nodes` or `mcp_open_nodes` to return 1-hop neighbor context for retrieved entities.

## Phase 3: Ergonomics & Lifecycle
*   **Task 3.1: Verbose Tool Responses**: Update `handle_tools_call` in `src/mcp.rs` to return the state of modified entities/relations instead of generic success messages.
*   **Task 3.2: Session Snapshotting**: Add a mechanism to record significant session state changes into a `SessionHistory` entity instead of only performing hard resets.

## Verification & Testing
- Add unit tests for entity normalization logic.
- Verify vector/FTS search integration via integration tests in `db.rs`.
- Ensure tool responses contain serialized entity state.
