# Excel spreadsheet (.xlsx) tools

Before modifying an Excel file, **only read what you actually need** — you need the exact sheet names (case-sensitive) and the current values of cells you intend to change. Do NOT load the entire workbook up front.

## Core principle: read what you touch, touch what you read

- Read the smallest range that gives you what you need to compute the next change.
- Apply changes in **small, logical batches** — one logical step at a time.
- A "logical step" is usually one of: create a sheet / add a header / fill one column / add a formula / fix one number / adjust formatting.
- Only batch unrelated changes when the user explicitly asks for a bulk edit.

## Read tools (pick the cheapest that answers your question)

| Tool | Returns | Use it when |
|---|---|---|
| `read_excel_metadata` | sheet names, merged ranges, formula addresses, used range | You don't know what's in the file yet |
| `read_excel_range` | values / formulas / styles for a specific `A1:D10` region | You know the exact range you'll edit |
| `read_office_file` | full sheets + 2-D values grid | Only when you genuinely need a full overview of a small workbook |
| `get_excel_info` | sheet count, cell count, formula count, per-sheet max row/col | Quick size check before deciding to read deeper |

Rule: start with `read_excel_metadata`. Only drill deeper into `read_excel_range` for the specific area of the next change.

## modify_excel (structured, incremental modification)

- Parameters: `path` (required), `operations` (required array, at least one entry).
- **A single `operations[]` should represent one logical step** — e.g. "fill column B with formulas" or "add a Summary sheet with header". Don't pre-pack changes from steps you haven't planned yet.
- Call `modify_excel` **once per logical step**. Re-read between steps if the next change depends on values just written.
- Atomic per call: written to a `.xlsx.tmp` sibling then renamed onto the target.
- **All cells you do not mention are preserved exactly as-is** — formulas, styles, number formats, charts, images.

**Operation types**:

| `type` | Purpose | Key fields |
|---|---|---|
| `modify_cell` | Change one cell | `sheet`, `address`, `value` / `formula`, `number_format`, `bg_color`, `font_*` |
| `write_range` | Write a 2-D array into a rectangular region | `sheet`, `start_cell`, `values[][]`, `number_format` |
| `merge_cells` | Merge or unmerge cells | `sheet`, `op` (`"merge"` \| `"unmerge"`), `start_cell`, `end_cell` |
| `resize_dimension` | Set row height / column width | `sheet`, `dimension` (`"row"` \| `"col"`), `index`, `size` |
| `sheet_op` | Manage sheets | `op` (create \| rename \| delete \| hide \| unhide), `sheet`, `new_name` |

## create_excel (build a brand-new workbook from scratch)

- Parameters: `path` (required), `sheets` (required array).
- Each sheet: `name` (required, 1-31 chars, unique in workbook), `cells` (optional), `merged` (optional).
- Each cell: `address` (A1 form, e.g. `"B3"`), `value` (`{type, value}` where `type ∈ empty | int | float | bool | string | datetime | error`), `formula` (optional, no leading `=`).
- If a file already exists at `path`, it is overwritten.
- **Prefer `modify_excel` over `create_excel` whenever the file already exists** — `create_excel` wipes everything, including cells you didn't touch.

## Critical constraints

- **Sheet names are case-sensitive** — copy exactly what `read_excel_metadata` returned.
- **Never use `write_file` for .xlsx** — it will corrupt the binary zip package.
- **Re-read before each new logical step** if the next change depends on values you just wrote or that may have shifted.

## Anti-patterns to avoid

- Reading the entire workbook just to change one cell.
- Building one mega-`operations[]` that mixes unrelated steps (create sheet + fill headers + add formulas + format columns) into a single call.
- Using `create_excel` to "update" an existing file.
- Skipping the re-read after a `write_range` that shifts data.

## Common recipes

**Change one cell**:
```json
[{"type": "modify_cell", "sheet": "Q1", "address": "B5",
  "value": {"type": "float", "value": 0.92}, "number_format": "0.00%"}]
```

**Write one column / one block of values**:
```json
[{"type": "write_range", "sheet": "Data", "start_cell": "A1",
  "values": [[{"type": "string", "value": "Name"}], [{"type": "string", "value": "Alice"}]]}]
```

**Create one new sheet + header row** (one logical step):
```json
[
  {"type": "sheet_op", "op": "create", "new_name": "Summary", "insert_index": 0},
  {"type": "modify_cell", "sheet": "Summary", "address": "A1",
   "value": {"type": "string", "value": "Monthly Report"}},
  {"type": "merge_cells", "sheet": "Summary", "op": "merge",
   "start_cell": "A1", "end_cell": "D1"}
]
```