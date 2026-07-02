# inkuo Sub-Agent Architecture

This directory contains the prompt-driven sub-agent system. Each markdown
file is a self-contained "expert" the main agent can delegate to via the
`delegate_to` tool.

## Layout

```
prompts/
├── main/
│   └── agent.slim.md         # Slim main-agent prompt (~500 tokens, tool names + 1-line descriptions)
├── ask.md                    # Ask Mode prompt (unchanged)
├── plan.md                   # Plan Mode prompt (unchanged)
├── edit.md                   # Edit Mode prompt (unchanged)
├── tool_specs/               # Detailed tool/group specs, loaded on-demand via `get_tool_help`
│   ├── general.md
│   ├── word.md
│   ├── excel.md
│   └── markdown.md
├── subagents/                # Sub-agent system prompts (this directory)
│   ├── office_word_expert.md
│   ├── office_excel_expert.md
│   ├── md_writer.md
│   ├── researcher.md
│   ├── batch_editor.md
│   └── code_expert.md
└── presets/                  # Future: paper / project workflows
```

## Adding a new sub-agent

Three changes:

1. **Write the prompt** at `prompts/subagents/<name>.md`. Include:
   - Profile name + English label (the label is what the UI shows).
   - The tool set this expert is allowed to call.
   - The workflow / decision rules for the expert.
   - Expected output format.

2. **Register the profile** in `src-tauri/src/agent/prompts.rs`:
   ```rust
   ProfileDescriptor {
       name: "my_new_expert",
       label: "My New Expert",
       system_prompt: include_str!("../../prompts/subagents/my_new_expert.md"),
       tools: &["read_file", "write_file", /* ... */],
       max_iterations: 15,
   },
   ```

3. **(Optional) Add a UI label translation** in `src/components/aipanel/toolUtils.ts`:
   ```ts
   const EXPERT_DISPLAY_NAMES: Record<string, string> = {
     // ...
     my_new_expert: '我的新专家',  // localized Chinese label for UI
   };
   ```

That's it — no Rust loop / registry edits needed. The dispatch table is
just `PROFILES.iter().find(...)`.

## How dispatch works

`delegate_to(expert, task)` is intercepted in `agent_loop::try_handle_meta_tool`.
The handler:

1. Looks up the profile by name (returns error if unknown).
2. Constructs a fresh `AgentSession` with:
   - The sub-agent's system prompt.
   - Filtered tool definitions (only `profile.allowed_tools`).
   - `max_iterations` from the profile (default 50).
3. Runs the sub-agent's loop using the **same** shared `ToolRegistry` (so
   `AppHandle` and lazily-registered tools like `database_search` propagate).
4. Returns the sub-agent's final summary as the tool result, wrapped in
   `[<label> completed]\n\n<summary>` for the main agent's transcript.

Sub-agent stream events arrive at the frontend under `message_id =
"sub:<expert>:<uuid>"` so they can be grouped / collapsed inside the
delegating call's card.

## Language convention

All prompt files are written in **English**, even though the product UI is
mostly Chinese. Reason: LLMs are tuned best on English instructions, and
mixing languages inside a prompt tends to degrade tool-call reliability and
code quality. UI-side labels stay in Chinese via the `EXPERT_DISPLAY_NAMES`
map in `toolUtils.ts`.

## Performance notes

Per-session token savings (rough estimates):

| Mode | Before | After | Savings |
|------|--------|-------|---------|
| General coding (no office) | ~6k | ~0.6k | **90%** |
| Word doc task | ~6k | ~0.6k + ~3k sub-agent | ~40% |
| Excel task | ~6k | ~0.6k + ~2.5k sub-agent | ~48% |

The biggest wins come from tasks that don't touch Office — those now hit
the slim 500-token prompt instead of the legacy 6k monolith.
