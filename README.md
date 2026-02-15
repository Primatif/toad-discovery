# toad-discovery

Ecosystem scanning and intelligence engine for the
[Primatif Toad](https://github.com/Primatif/Primatif_Toad) ecosystem.

## What It Does

`toad-discovery` is the **scanning brain** of Toad. It walks your filesystem,
discovers projects, extracts metadata, and builds the structured registry that
powers every other Toad feature.

- **Project Scanning** — Recursively discovers projects by detecting stack
  markers (Cargo.toml, package.json, go.mod, etc.) using the `StackStrategy`
  system defined in `toad-core`.
- **Metadata Extraction** — For each project: stack detection, activity tier
  classification, VCS status analysis, tag discovery, sub-project detection, and
  semantic essence extraction from READMEs.
- **Registry Sync** — `sync_registry()` orchestrates a full ecosystem scan,
  generates the `EcosystemChangelog`, and persists the `ProjectRegistry` to
  `~/.toad/shadows/`.
- **Status Reports** — `generate_status_report()` produces structured
  `StatusReport` data with per-project health, git status, and submodule
  alignment — filterable by query and tag.
- **Semantic Search** — `search_projects()` searches across project names,
  essence, tags, and taxonomy for AI-powered context retrieval.

## Role in the Ecosystem

`toad-discovery` is the primary data producer. It depends on `toad-core` (data
models), `toad-git` (VCS analysis), and `toad-ops` (stats). Its output feeds
into `toad-manifest` (context generation), the CLI (`toad status`, `toad sync`),
and the MCP server (`list_projects`, `search_projects` tools).

```text
toad-core ──┐
toad-git  ──┼── toad-discovery ──┬── toad-manifest
toad-ops  ──┘                    ├── bin/toad (CLI)
                                 └── bin/toad-mcp (MCP server)
```

## License

BUSL-1.1
