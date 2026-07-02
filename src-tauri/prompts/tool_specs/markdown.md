# Markdown / plain-text editing

Markdown editing only uses the general file tools — there are no dedicated Markdown-specific tools:

- `read_file` — read .md
- `write_file` — create / overwrite
- `edit_file` — precise local edit (tweak a paragraph, change frontmatter, etc.)

## Workflow for writing long Markdown

1. First produce an outline (≤ 8 section lines) and confirm with the user.
2. Write each section with `write_file` (or append via `edit_file` to the end of the document).
3. Keep each chapter / section around 1500-2000 characters so a single call doesn't blow up token count.
4. When the document is complete, re-read it once and smooth over section transitions.

## Markdown formatting conventions

- First line: `# H1` (the document title).
- Use `## H2` and `### H3` for subsections as appropriate.
- Lists, tables, code blocks: use GitHub-flavored Markdown syntax.
- Reference links as `[text](url)`, not bare URLs.
- Don't add emoji unless the user explicitly requests it.

## Watch-outs

- **Don't fit a long document into a single `edit_file` `old_text`** — that explodes token count. Use `write_file` to create the whole file, then `edit_file` for targeted fixes.
- **For frontmatter changes use `edit_file`** — preserve the original YAML structure, only modify the field you need.
