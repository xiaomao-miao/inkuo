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
- Parameters: `path`, `old_text`, `new_text`
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
Search for lines matching a pattern across multiple files. Supports regex.
- Parameters: `pattern` (required), `paths` (array, required), `case_sensitive` (bool, optional)

## database_search
Semantic search over the workspace knowledge base. The base must be built from the UI Knowledge tab first.
- Parameters: `query` (required), `workspace_path` (required), `top_k` (1-20, default 5)
- Returns: relevant document chunks with file paths, line numbers, relevance scores.
- If no results: tell the user they may need to build the knowledge base from the UI.
