# General file / search tools

## read_file
Read the entire content of a text file. For large files, locate them first with `list_dir` / `glob`, then read in chunks using `offset` / `limit`.
- Parameters: `path` (required), `offset` (int, optional), `limit` (int, optional)

## write_file
Create a new file or fully overwrite an existing one. Parent directories are created automatically.
- Parameters: `path` (required), `content` (required)
- Note: **never use `write_file` for .xlsx** — xlsx is a binary zip package; use `create_excel` / `modify_excel` instead.

## edit_file
Replace an exact text snippet with another. `old_text` must match **exactly** — including whitespace, newlines, and comments.
- Parameters: `path`, `old_text`, `new_text`, `replace_all` (bool, optional, default false)
- By default `old_text` must match exactly once; passing `replace_all=true` substitutes every occurrence (useful for renames).
- On failure (`old_text not found`): re-read the file and copy the exact text.
- Use `edit_file` for precise small changes; use `write_file` for new files or full rewrites.

## create_dir
Create a directory (parent directories created automatically).
- Parameters: `path` (required)

## move_file
Move or rename a file / directory.
- Parameters: `src` (source path), `dst` (destination path)

## list_dir
List files / subdirectories in a directory.
- Parameters: `path` (required)

## glob
Find files by glob pattern (e.g. `**/*.md`, `docs/**/*.ts`).
- Parameters: `pattern` (required), `base_dir` (required)

## grep
Search for lines containing a **literal substring** (case-insensitive by default) across multiple files. This is plain substring matching — NOT regex.
- Parameters: `pattern` (required, literal substring), `paths` (array, required), `case_sensitive` (bool, optional)
- For true regex / advanced queries, delegate to `code_expert`.

## database_search
Semantic search over the workspace knowledge base. The base must be built from the UI Knowledge tab first.
- Parameters: `query` (required), `top_k` (1-20, default 5)
- Returns: relevant document chunks with file paths, line numbers, relevance scores.
- The active workspace is determined by the registry — there is no `workspace_path` parameter to set.
- If no results: tell the user they may need to build the knowledge base from the UI.
