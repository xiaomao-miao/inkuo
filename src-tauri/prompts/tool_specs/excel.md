# Excel spreadsheet (.xlsx) tools

Before modifying an Excel file, **almost always call a read tool first** — you need the exact sheet names (case-sensitive) and current structure.

## Standard workflow (4 steps)
1. `read_excel_metadata` — sheet names + merged ranges + formulas (cheapest).
2. `read_excel_range` — values / formulas / styles for the specific area you need.
3. `modify_excel` with `operations[]` — submit all changes atomically in one call.
4. Verify.

## create_excel (build from scratch)
- Parameters: `path` (required), `sheets` (required array).
- Each sheet: `name` (required, 1-31 chars, unique in workbook), `cells` (optional), `merged` (optional).
- Each cell: `address` (A1 form, e.g. `"B3"`), `value` (`{type, value}` where `type ∈ empty | int | float | bool | string | datetime | error`), `formula` (optional, no leading `=`).
- If a file already exists at `path`, it is overwritten.

## modify_excel (structured batch modification — unified entry point for all Excel edits)
- Parameters: `path` (required), `operations` (required array, at least one entry).
- A single `operations` array can mix multiple operation types.
- **All cells you do not mention are preserved exactly as-is** — formulas, styles, number formats, charts, images.
- Atomic: written to a `.xlsx.tmp` sibling then renamed onto the target.

**Operation types**:

| `type` | Purpose | Key fields |
|---|---|---|
| `modify_cell` | Change one cell | `sheet`, `address`, `value` / `formula`, `number_format`, `bg_color`, `font_*` |
| `write_range` | Write a 2-D array into a rectangular region | `sheet`, `start_cell`, `values[][]`, `number_format` |
| `merge_cells` | Merge or unmerge cells | `sheet`, `op` (`"merge"` \| `"unmerge"`), `start_cell`, `end_cell` |
| `resize_dimension` | Set row height / column width | `sheet`, `dimension` (`"row"` \| `"col"`), `index`, `size` |
| `sheet_op` | Manage sheets | `op` (create \| rename \| delete \| hide \| unhide), `sheet`, `new_name` |

## read_excel_range (efficient range read)
- Parameters: `path`, `sheet` (case-sensitive), `range` (A1 form, e.g. `"A1:D10"`), `include_styles` (optional comma-separated list).
- Use this instead of `read_office_file` when you only need a portion of a sheet — far more token-efficient.

## read_excel_metadata (cheapest overview)
Does NOT return cell values. Returns sheet names, merged ranges, used range, cell count, formula count, and per-sheet formula addresses.

## get_excel_info
Read .xlsx summary (sheet count, total cells, total formulas, per-sheet max row / column) — does NOT return values.

## read_office_file (Excel branch)
For .xlsx inputs returns `sheets` (sheet name list), `sheets_summary` (per-sheet dimensions), and `values` (2-D string grid; formula cells rendered as `=...`).

## Critical constraints
- **Sheet names are case-sensitive** — copy exactly what `read_excel_metadata` returned.
- **Never use `write_file` for .xlsx** — it will corrupt the binary zip package.
- **`modify_excel` is atomic** — pack every change you can into one `operations` array.

## Common recipes

**Fix a single number + format**:
```json
[{"type": "modify_cell", "sheet": "Q1", "address": "B5",
  "value": {"type": "float", "value": 0.92}, "number_format": "0.00%"}]
```

**Write a table in one shot**:
```json
[{"type": "write_range", "sheet": "Data", "start_cell": "A1",
  "values": [[{"type": "string", "value": "Name"}, {"type": "int", "value": 30}]]}]
```

**Create a new sheet with a merged header**:
```json
[
  {"type": "sheet_op", "op": "create", "new_name": "Summary", "insert_index": 0},
  {"type": "modify_cell", "sheet": "Summary", "address": "A1",
   "value": {"type": "string", "value": "Monthly Report"}},
  {"type": "merge_cells", "sheet": "Summary", "op": "merge",
   "start_cell": "A1", "end_cell": "D1"}
]
```
