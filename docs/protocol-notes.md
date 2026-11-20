# Protocol notes

What the real `claude` program did when driven through `mindrune-probe` on 2026-11-10. Tasks 10 to 16 of plan 1 and plans 2 to 5 treat this file as the source of truth. Where it disagrees with an earlier assumption, this file wins. Two sections were added later: "Stored sessions", by plan 3, from the store on disk rather than from the probe, and the last, "Read in the program for plan 5", from the program's own text. Nothing in that last section was seen live, and it marks each point as seen, read in the program or guessed. "The context breakdown" followed plan 5, and plan 6 added "Attachments, as sent", the shapes the adapter writes for a message with images and files, marked the same way.

Lines without a prefix below are what `claude` sent. They come from the scrubbed fixtures in `crates/claude/tests/fixtures/live/`, with some fields left out for brevity and long strings cut with `…`; the fixtures hold the full records. Lines starting with `> ` are what the probe sent. Those come from the raw recordings and are in no fixture, because the scrubber keeps only what `claude` sent. In the fixtures, UUIDs are replaced with numbered placeholders, the e-mail and home path are replaced, lists longer than six entries are cut to three, the MCP server and plugin lists are cut to one, thinking signatures are replaced, the `get_usage` reply keeps only its `five_hour` and `seven_day` windows and drops its `behaviors` block, and one file path that the stream had split across two lines was fixed by hand.

## Machine notes

- `claude` version: `2.1.319`. Rust `1.99.1`. Ubuntu, Linux 7.0.
- Task 2: Rune built with no missing system libraries. `--snap` and `--tree` rendered headless on the first try with no GPU fallback, inside the agent sandbox.
- The probe was run in the user's own terminal, never in an agent shell. An agent shell cannot see the saved login.
- The probe was driven through a named pipe (`target/probe-in`) so commands could be typed by the controller while the recording was followed from another process.
- Every session used `--model haiku --full-login` and the working directory `target/probe-cwd`.
- Side effects of the run, outside this repository: session transcripts under `~/.claude/projects/` for the probe's working directory, and one plan file under `~/.claude/plans/` written by `claude` itself in V7. The always-allow rule from V3 was written to `<git root>/.claude/settings.local.json`; that file was removed after the run.

## Summary of differences from the plan

| Item | Difference | Task whose rules or test data change |
| --- | --- | --- |
| V2, V3 | A request carries several `permission_suggestions` of different kinds. Sending them all back would also switch the session to `acceptEdits`. Always-allow must send only the suggestion the label describes. | 12 |
| V3 | The rule is saved in the git root's `.claude/settings.local.json`, not in the working directory's. | none (wording in plan 2) |
| V5 | `set_model` answers with the resolved model id, and also emits a `user` record whose `message.content` is a string. The model that answers a turn can differ from the one `init` names. | 10, 13 |
| V6 | A stopped turn ends in a `result` with `is_error: true`, no `result` text, and `terminal_reason` `aborted_streaming` or `aborted_tools`. It must not be shown as a failure. | 10, 11 |
| V6, V7 | `control_cancel_request` withdraws a pending permission request. New record type. | 10, 12, and one new event in `mr-agent` |
| V7 | `system`/`status` carries `permissionMode` whenever the mode changes, including after a plan is approved. | 10, 11 |
| V4 | Thinking arrives with empty text: every `thinking_delta` has `"thinking":""`. Empty deltas must not be passed on. | 11 |
| V1 | Bridge states seen: `ready`, `connected`. The `remote_control` reply carries `session_url`. | 13 |
| V9 | `rename_session` is the right request and it works. `session_title_changed` arrives before the reply. | 7 (marker comment only), 13 |
| V9 | `get_usage`, `get_context_usage` and `list_models` all work. Not used by plan 1. | none (plans 2 to 4) |

## V1 Remote Control

Sent after one finished turn: the probe command `rc on`.

```json
> {"request":{"enabled":true,"subtype":"remote_control"},"request_id":"probe_2","type":"control_request"}
```

Came back, in this order (a `system`/`background_tasks_changed` record also arrived between the first and second line):

```json
{"type":"system","subtype":"bridge_state","state":"ready","session_id":"00000000-0000-4000-8000-000000000002"}
{"type":"control_response","response":{"subtype":"success","request_id":"probe_2","response":{"session_url":"https://claude.ai/code/session_SCRUBBED","connect_url":"https://claude.ai/code?environment=","environment_id":"","bridge_epoch":1,"bridge_session_id":"cse_SCRUBBED"}}}
{"type":"system","subtype":"bridge_state","state":"connected","bridge_epoch":1,"session_id":"00000000-0000-4000-8000-000000000002"}
```

- `bridge_state` values in order: `ready`, then `connected`. The reply to the request arrives between them.
- The reply's body holds `session_url` (the page on claude.ai for this session), `connect_url`, `environment_id`, `bridge_epoch` and `bridge_session_id`. With a non-empty `environment_id`, `connect_url` is an address of the session too. Since the fix wave after plan 5's review the probe's `raw` mode shows the values of `session_url`, `connect_url` and `bridge_session_id` as `<hidden>` in the terminal, wherever in a line they sit; its recording keeps them.
- A turn sent while connected ran normally.

Sent: `rc off`. Came back: only the reply. No `bridge_state` line follows.

```json
{"type":"control_response","response":{"subtype":"success","request_id":"probe_3"}}
```

Turning it on a second time in the same session worked the same way, with `bridge_epoch` 2 and, in the raw recording, a different `session_url`.

- The `initialize` reply carries `remote_control_available: true`.
- **Not yet confirmed by a person:** that the session shows up at claude.ai/code. The controller could not check (not signed in to claude.ai in its browser) and Jerod was away. The protocol evidence above was accepted as passing the gate. Plan 5 built the switch on it; the check by a person is now two steps in `docs/notes-for-later-plans.md`, "Jerod's look at plan 5".
- The `failed` state and the `remote_control_auth` error code were seen on 2026-11-10 before the full login existed, when only the token sign-in was present. They are not in these fixtures.
- The other states, the other refusal codes and what the adapter gives for each were read in the program for plan 5: see "Read in the program for plan 5".

Fixture: `v1-remote.jsonl`. Differs from the plan: no. Task 13 gets the state names `ready` (starting) and `connected` (on), and takes the URL from the reply.

## V2 Permission round trip

Sent: `say Use the Bash tool to run: echo rune-claude-probe`. No request arrived, because the user's own settings already allow it. The tool result says so:

```json
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01RK5qmpr2PATPM8C1D4E3Ra","type":"tool_result","content":"rune-claude-probe","is_error":false}]},"tool_result_meta":[{"id":"toolu_01RK5qmpr2PATPM8C1D4E3Ra","permission_decision":{"decision":"accept","source":"config","reason_type":"subcommandResults"}}]}
```

Sent: `say Use the Bash tool to run exactly: touch probe-marker.txt`. Came back:

```json
{"type":"control_request","request_id":"00000000-0000-4000-8000-000000000045","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"touch probe-marker.txt","description":"Create empty probe-marker.txt file"},"description":"Create empty probe-marker.txt file","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch probe-marker.txt"}],"behavior":"allow","destination":"localSettings"},{"type":"addDirectories","directories":["/home/user/git/rune_claude/target/probe-cwd"],"destination":"session"},{"type":"setMode","mode":"acceptEdits","destination":"session"}],"blocked_path":"/home/user/git/rune_claude/target/probe-cwd/probe-marker.txt","tool_use_id":"toolu_018R9NpWTRq5sxDPoWGaojLQ"}}
```

- A request has `request_id`, and inside `request`: `subtype`, `tool_name`, `display_name`, `input`, `description`, `permission_suggestions`, `blocked_path`, `tool_use_id`. Other requests also carried `decision_reason` and `decision_reason_type` (V7), or `requires_user_interaction` (V7).
- `permission_suggestions` here has three entries of three kinds: `addRules` to `localSettings`, `addDirectories` to `session`, and `setMode` `acceptEdits` to `session`.

Answered `allow <id>`. The probe sent, and the tool then ran:

```json
> {"response":{"request_id":"<id>","response":{"behavior":"allow","updatedInput":{"command":"touch probe-marker.txt","description":"Create empty probe-marker.txt file"}},"subtype":"success"},"type":"control_response"}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_018R9NpWTRq5sxDPoWGaojLQ","type":"tool_result","content":"(Bash completed with no output)","is_error":false}]},"tool_result_meta":[{"id":"toolu_018R9NpWTRq5sxDPoWGaojLQ","permission_decision":{"decision":"accept","source":"user_temporary"}}]}
```

Repeated with another file name and answered `deny <id>`. Claude received the message as a failed tool result:

```json
> {"response":{"request_id":"<id>","response":{"behavior":"deny","message":"Denied from the probe"},"subtype":"success"},"type":"control_response"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"Denied from the probe","is_error":true,"tool_use_id":"toolu_019zfPtQQxrFY6AbobgRc2Pi"}]},"tool_use_result":"Error: Denied from the probe","tool_result_meta":[{"id":"toolu_019zfPtQQxrFY6AbobgRc2Pi","non_execution_kind":"permission-rule","permission_decision":{"decision":"reject","source":"user_reject"}}]}
```

Fixture: `v2-permission.jsonl`. Differs from the plan: the request shape is as assumed, with extra fields. The reply shapes are as assumed.

## V3 Always allow

Repeated with a third file name. Answered with a raw line that sends back **only the `addRules` suggestion**, not all three:

```json
> {"type":"control_response","response":{"subtype":"success","request_id":"<id>","response":{"behavior":"allow","updatedInput":{"command":"touch probe-marker-3.txt","description":"Create empty probe-marker-3.txt file"},"updatedPermissions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch probe-marker-3.txt"}],"behavior":"allow","destination":"localSettings"}]}}}
```

The tool ran. The same message sent a fourth time produced no request, and the tool result names the saved rule as the reason:

```json
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01MRSLzsjytZEoKktWWvpvsh","type":"tool_result","content":"(Bash completed with no output)","is_error":false}]},"tool_result_meta":[{"id":"toolu_01MRSLzsjytZEoKktWWvpvsh","permission_decision":{"decision":"accept","source":"user_permanent","reason_type":"rule"}}]}
```

- The rule was written to `<git root>/.claude/settings.local.json` as `"permissions": {"allow": ["Bash(touch probe-marker-3.txt)"]}`. Nothing appeared under `target/probe-cwd/.claude/`. "Project" for `localSettings` means the git root, not the session's folder.
- The probe's own `always` command was deliberately not used. At the time it echoed every suggestion, and the third one would have switched the session to `acceptEdits`. It now sends only the first, as the adapter does.

Fixture: `v2-permission.jsonl`. **Differs from the plan: yes.** Task 12 said always-allow sends back all remembered suggestions. It must send only the first suggestion, the one `always_allow_label` describes. Task 12's rule and its test for `Allow { always: true }` change.

## V4 Image

Sent: `image target/probe-cwd/red.png What colour is this image? One word.` (a 16×16 solid red PNG). The user message carried a text block and an `image` block with a base64 source. The answer was `Red`.

The reply began with a thinking block. Thinking arrives with empty text and a signature, both in the stream and in the complete record:

```json
{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}}
{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"","signature":"scrubbed-signature"}]}}
```

Inside the block the stream carries `thinking_delta` events whose text is always empty, then one `signature_delta`. Every thinking block in the fixtures looks like this. Progress is reported separately by `system`/`thinking_tokens` records:

```json
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"","estimated_tokens":null}}}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"scrubbed-signature"}}}
{"type":"system","subtype":"thinking_tokens","estimated_tokens":50,"estimated_tokens_delta":50,"session_id":"00000000-0000-4000-8000-000000000001"}
```

Fixture: `v4-image-model-stop.jsonl`. **Differs from the plan: yes, for thinking.** Task 11 must drop deltas whose text is empty, must not emit `BlockFinished` for a block that passed on no text, and must not emit an empty `ThinkingDelta` from a complete record.

## V5 Model change

Sent: `model sonnet`. Came back, before any new turn:

```json
> {"request":{"model":"sonnet","subtype":"set_model"},"request_id":"probe_2","type":"control_request"}
{"type":"user","message":{"role":"user","content":"<local-command-stdout>Set model to `sonnet (claude-sonnet-5-5)`</local-command-stdout>"},"session_id":"00000000-0000-4000-8000-000000000001","parent_tool_use_id":null,"timestamp":"2026-11-11T06:48:40.710Z","isReplay":true}
{"type":"control_response","response":{"subtype":"success","request_id":"probe_2","response":{"model":"claude-sonnet-5-5"}}}
```

- The reply's body holds the resolved model id.
- A `user` record with `isReplay: true` also arrives, and its `message.content` is a **string**, not a list of blocks.
- The next turn's `system`/`init` shows `"model":"claude-sonnet-5-5"`, and so do the `message_start` event and the `assistant` records.
- `init` names the model that is selected. It is not always the model that answers. In V7's plan-mode turn the `init` said `claude-haiku-5-5` while `message_start` and every `assistant` record of that turn said `claude-sonnet-5-5`; after the plan was approved they said haiku again. No `set_model` was sent. Plan 1 reports the selected model (from `init` and from `set_model` replies). The answering model is in `assistant.message.model`, for plan 2 to show per message if wanted.

Fixture: `v4-image-model-stop.jsonl`. **Differs from the plan: yes.** Task 10 must treat a `user` record with no `tool_result` blocks, including string content, as ignored. Task 13 should report the resolved id from the reply.

## V6 Stop

Sent a request for a long essay, then `stop` as soon as text started streaming. Came back:

```json
> {"request":{"subtype":"interrupt"},"request_id":"probe_5","type":"control_request"}
{"type":"control_response","response":{"subtype":"success","request_id":"probe_5","response":{"still_queued":[]}}}
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}
{"type":"result","subtype":"error_during_execution","is_error":true,"stop_reason":null,"terminal_reason":"aborted_streaming","errors":["[ede_diagnostic] result_type=user last_content_type=n/a stop_reason=null"],"duration_ms":3555,"total_cost_usd":0.06458246000000001,"num_turns":2}
```

- The `result` record of a stopped turn has `subtype` `error_during_execution`, `is_error` `true`, `stop_reason` `null`, **no `result` field**, an `errors` list, and `terminal_reason` `aborted_streaming`.
- A turn stopped while a permission request was pending (V7) ends the same way with `stop_reason` `tool_use` and `terminal_reason` `aborted_tools`.
- A normal turn has `terminal_reason` `completed`.
- An `interrupt` sent after the turn had already finished got the same success reply and nothing else.
- The next message after a stop was answered normally.

Fixture: `v4-image-model-stop.jsonl`. **Differs from the plan: yes.** Tasks 10 and 11 treated `is_error` as failure. A `result` whose `terminal_reason` starts with `aborted` is a stopped turn and ends the turn without a failure. Task 10's `Result` gains a field for it, and its text falls back to `errors` when `result` is missing.

## V7 Questions and plans

Sent: `say Ask me one multiple-choice question with your question tool before doing anything else.` Came back:

```json
{"type":"control_request","request_id":"00000000-0000-4000-8000-000000000060","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","display_name":"AskUserQuestion","input":{"questions":[{"question":"What would you like to focus on next in this repo (branch plan-1-foundations)?","header":"Next step","options":[{"label":"Continue foundations work","description":"Pick up the next planned foundations step after the recent probe commits."},{"label":"Review recent commits","description":"Review the last five commits (child process, encoding, probe and scrubber) for bugs."},{"label":"Run the test suite","description":"Build and run the tests to check the current state of the branch."},{"label":"Something else","description":"I'll describe a different task."}],"multiSelect":false}]},"tool_use_id":"toolu_012cyCMWSCcg1PfyuL36gPXz","requires_user_interaction":true}}
```

Answered with a raw line: `behavior` `allow`, and `updatedInput` equal to the request's `input` plus `"answers": {"<the question text>": "<the chosen option's label>"}`. Claude received:

```json
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"Your questions have been answered: \"What would you like to focus on next in this repo (branch plan-1…","tool_use_id":"toolu_012cyCMWSCcg1PfyuL36gPXz"}]}}
```

Sent: `mode plan`. Came back:

```json
> {"request":{"mode":"plan","subtype":"set_permission_mode"},"request_id":"probe_3","type":"control_request"}
{"type":"control_response","response":{"subtype":"success","request_id":"probe_3","response":{"mode":"plan"}}}
{"type":"system","subtype":"status","status":null,"permissionMode":"plan","session_id":"00000000-0000-4000-8000-000000000001"}
```

Sent: `say Plan how to add a README here, then ask to leave plan mode. Do not read any files first.` Came back:

```json
{"type":"control_request","request_id":"00000000-0000-4000-8000-000000000364","request":{"subtype":"can_use_tool","tool_name":"ExitPlanMode","display_name":"ExitPlanMode","input":{"plan":"# Plan: Add a README\n\n## Context\nThe repo (`rune_claude`, branch `plan-1-foundat…","planFilePath":"/home/user/.claude/plans/plan-how-to-add-silly-frost.md"},"tool_use_id":"toolu_016pZCgGXgkiknGiDrXfv274","requires_user_interaction":true}}
```

Answered `allow <id>`. Came back, in this order:

```json
{"type":"system","subtype":"status","status":null,"permissionMode":"default","session_id":"00000000-0000-4000-8000-000000000001"}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"User has approved your plan. You can now start coding. Start with updating your todo list if applica…","tool_use_id":"toolu_016pZCgGXgkiknGiDrXfv274"}]}}
```

Stopping a turn while a permission request was waiting produced a new record type, before the reply to the interrupt:

```json
{"type":"control_cancel_request","request_id":"00000000-0000-4000-8000-000000000151"}
```

- A question arrives as `can_use_tool` with `tool_name` `AskUserQuestion`, `requires_user_interaction: true` and no `permission_suggestions`. `input.questions[]` has `question`, `header`, `options[]` (`label`, `description`) and `multiSelect`.
- A plan arrives as `can_use_tool` with `tool_name` `ExitPlanMode`. `input` has `plan` (markdown) and `planFilePath`.
- Approving a plan switches the mode back, reported by `system`/`status` with `"status":null,"permissionMode":"default"`.
- `system`/`status` with `"status":"requesting"` and no `permissionMode` is sent at the start of every model request.
- Requests can carry `decision_reason` and `decision_reason_type`. Two Bash requests had them and no `permission_suggestions`. A `Read` request outside the working folder had them and one suggestion, `addRules` to `session`.

Fixture: `v7-question-plan.jsonl`. Differs from the plan: the question and plan shapes are as assumed. **New:** `control_cancel_request` (Tasks 10 and 12, plus a new event `RequestWithdrawn` in `mr-agent`), and `system`/`status` carrying the mode (Tasks 10 and 11).

## V8 Is `init` sent every turn?

Yes. A `system`/`init` record is sent at the start of every turn, not once per process. It is the first record after each user message. Its fields:

```json
{"type":"system","subtype":"init","session_id":"00000000-0000-4000-8000-000000000002","model":"claude-haiku-5-5","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/home/user/git/rune_claude/target/probe-cwd","apiKeySource":"none","output_style":"default"}
```

Other keys present: `additional_directories`, `agents`, `analytics_disabled`, `capabilities`, `fast_mode_disabled_reason`, `fast_mode_state`, `mcp_servers`, `memory_paths`, `messaging_socket_path`, `original_cwd`, `per_turn_effort_active`, `plugins`, `product_feedback_disabled`, `skills`, `slash_commands`, `terminal_slash_commands`, `tools`, `ultrathink_available`, `ultrathink_typed_raises`, `uuid`, `view_mode`.

Before the first turn, nothing but the reply to `initialize` (and `system`/`commands_changed`) is sent. The session id is not known from output until the first turn's `init`; the adapter knows it earlier because it chose it with `--session-id`. The adapter therefore reports a session as started when the reply to `initialize` arrives, using its own id and the reply's `models`, `current_permission_mode` and `claude_code_version`.

Differs from the plan: no. Task 11's "a later `Init` reports only what changed" rule is needed on every turn.

## V9 Other control requests

Each was sent raw with ids `p1` to `p4` and `p6`; the working rename was sent with the probe's own `rename` command as `probe_2`.

`get_usage` works. Selected keys of its reply are shown. `utilization` here is a **percentage** (37 means 37%), and `resets_at` is an ISO 8601 time. In `rate_limit_event` the same numbers are a fraction (`0.33`) and a Unix time.

```json
{"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":37,"resets_at":"2026-11-11T08:19:59.618411+00:00","limit_dollars":null,"used_dollars":null,"remaining_dollars":null,"locked_reason":null},"seven_day":{"utilization":10,"resets_at":"2026-11-15T20:59:59.618435+00:00","limit_dollars":null,"used_dollars":null,"remaining_dollars":null,"locked_reason":null}}}
```

For comparison, the event sent during a turn:

```json
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1794385200,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":0.33,"resetsAt":1794385200,"status":"allowed"},"seven_day":{"utilization":0.09,"resetsAt":1794776400,"status":"allowed"}}},"session_id":"00000000-0000-4000-8000-000000000002"}
```

`get_context_usage` works. Its reply has, among other keys:

```json
{"totalTokens":27212,"maxTokens":1000000,"percentage":3,"model":"claude-haiku-5-5","autoCompactThreshold":967000,"isAutoCompactEnabled":true}
```

`list_models` works. Each entry looks like this. The same list is in the `initialize` reply under `models`; since plan 5 the adapter reads `supportsEffort` and `supportedEffortLevels` from it.

```json
{"value":"default","resolvedModel":"claude-fable-5-2","displayName":"Default (recommended)","description":"Fable 5.2","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"],"supportsAdaptiveThinking":true,"supportsAutoMode":true,"supportsThinkingOff":false}
```

Renaming: `rename` and `set_title` are refused. `rename_session` with a `title` works, and the title change is announced **before** the reply:

```json
{"type":"control_response","response":{"subtype":"error","request_id":"p4","error":"Unsupported control request subtype: rename"}}
> {"request":{"subtype":"rename_session","title":"Probe rename B"},"request_id":"probe_2","type":"control_request"}
{"type":"system","subtype":"session_title_changed","title":"Probe rename B","session_id":"00000000-0000-4000-8000-000000000001"}
{"type":"control_response","response":{"subtype":"success","request_id":"probe_2"}}
```

The reply to `initialize` is large. Keys: `commands`, `agents`, `output_style`, `available_output_styles`, `user_output_styles_dir`, `models`, `account`, `pid`, `claude_code_version`, `current_permission_mode`, `hook_on_failure_supported`, `feedback_mode`, `analytics_disabled`, `remote_control_auto_enable`, `remote_control_auto_connect_default`, `remote_control_available`, `remote_control_auto_on_by_default`, `ide_rc_auto_enable_gate`, `fast_mode_state`, `fast_mode_disabled_reason`, `session_state`, `capabilities`. Its `account` object:

```json
{"email":"user@example.com","organization":"user@example.com's Organization","subscriptionType":"Claude Max","apiProvider":"firstParty"}
```

Other record types seen and not used by plan 1: `system`/`commands_changed`, `system`/`background_tasks_changed` (read since plan 7: see "Background tasks"), `system`/`post_turn_summary`, `system`/`thinking_tokens`, `system`/`status`, and the `system`/`hook_*` records the scrubber drops.

Fixture: `v9-control.jsonl`. Differs from the plan: no. The guessed `rename_session` shape in Task 7 is correct, so its "unverified" marker comment can go. Task 13 must not report the same title twice.

## V10 Sign-in status

`env -u CLAUDE_CODE_OAUTH_TOKEN claude auth status` printed, with the e-mail, organisation id and name replaced:

```json
{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "analyticsDisabled": false,
  "projectsDirectory": "/home/user/.claude/projects",
  "configDirectory": "/home/user/.claude",
  "loginExpiresAt": "2026-12-08T18:53:18.026Z",
  "email": "user@example.com",
  "orgId": "00000000-0000-4000-8000-000000000001",
  "orgName": "user@example.com's Organization",
  "subscriptionType": "max",
  "storedClaudeAiLogin": true,
  "planTier": "max_5x"
}
```

With only the token (seen earlier on 2026-11-10): `{"loggedIn": true, "authMethod": "oauth_token", "apiProvider": "firstParty"}`. Signed out: `{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}`.

Differs from the plan: no. Task 14's field names `email` and `subscriptionType` are right. `planTier` is also available.

## V11 Resume and fork

Started the probe again with `--resume <id>` of the V9 session. The title was announced before the reply to `initialize`. The first turn's `init` had the same `session_id`, and a question about a word given in the earlier session was answered correctly.

```json
{"type":"system","subtype":"session_title_changed","title":"Probe rename B","session_id":"00000000-0000-4000-8000-000000000002"}
```

Started again with `--resume <id> --fork`. The `init` had a **new** `session_id`. The raw recordings show two different ids; the scrubber numbers ids per file, so the fixtures cannot show it. The title was carried over, and the earlier conversation was still remembered.

Checked later the same day, by hand: `claude -p --resume <old> --fork-session --session-id <new>` gives the fork the id `<new>`. So a fork's id can be chosen up front, like a new session's. The adapter does this. (`--session-id` together with `--resume` is accepted only with `--fork-session`.)

Fixtures: `v11-resume.jsonl`, `v11-fork.jsonl`. Differs from the plan: no for resume. For a fork, Task 5's arguments gained `--session-id`.

## Stored sessions

Added by plan 3 ("many sessions"), which lists the sessions Claude Code keeps on disk and reads their history. Checked on 2026-11-13 against the real store of the machine this was built on (790 files), by key names and shapes only, never by content.

### Where they are

Each stored session is one file of JSON lines, `~/.claude/projects/<folder>/<session id>.jsonl`, where `<folder>` is the session's working folder with every slash made a dash. Since plan 8 the adapter looks in `$CLAUDE_CONFIG_DIR` in place of `~/.claude` when that variable is set and not empty, as the program does. The adapter only reads there, except that it removes one session for good when asked (see "A session's files, and removing them"). A session with no message stores nothing, so a session that was started and closed without a message (a look at launch, for example) leaves no file. The file's modification time is when the session last changed: the listing orders by it, and the app treats a session whose file changed less than two minutes ago as possibly open in another app, unless the change came no later than ten seconds after this app last closed that session (the program's own last writes as it exits).

### What the records say

- Records that carry conversation have `type` `user` or `assistant`, a `uuid` and a `parentUuid`. Other record types (`system`, `attachment`) also have `uuid` and `parentUuid`, and a `user` or `assistant` record's parent is often one of those. So the chain must be followed through every record that has a `uuid`, not only through `user` and `assistant`.
- A file can hold abandoned branches: two records with the same `parentUuid`. The conversation is the chain that ends at the last `user` or `assistant` record of the file that is not a side chain (`isSidechain: true`). The file's order is not the conversation's order, and a record's parent is not always the line before it.
- Compaction writes a `system` record with `subtype: "compact_boundary"`, `parentUuid: null` and `logicalParentUuid` naming the record before it. Following `logicalParentUuid` keeps the chain whole across it. It is followed by a `user` record with `isCompactSummary: true` (also `isVisibleInTranscriptOnly: true`) whose text is the summary; that record must give no item, or the summary would appear as something the user typed. The app shows "Earlier messages were summarised here to save room." in its place.
- The effort level of a reply is `effort`, a top-level string on each `assistant` record, not under `message` (checked on 2026-11-15 by key shapes, in one stored session of 2,455 `assistant` records). Plan 5 takes the `effort` of the last `assistant` record on the chain that has one as the level a stored session goes on with.
- The model that answered is `message.model` on an `assistant` record. A program-made message (for example after an interrupted turn) has the model `<synthetic>`, which is skipped when the session's model is read from its history.
- A record `{"type":"permission-mode","permissionMode":"auto","sessionId":…}` is written when the mode changes. The names are the wire names (`default`, `acceptEdits`, `plan`, `auto`, `bypassPermissions`). These records have no `uuid`. The app takes Plan, Accept edits and Auto from a history, but never `bypassPermissions` or a name it does not know: such a session wakes in the program's default mode.
- Records written during a turn carry `entrypoint`: how the program was started. Seen in the real store on 2026-11-14: `cli` (the terminal program), `claude-desktop`, `sdk-cli` (this app, and `claude -p`), and `sdk-ts` (a program driving the TypeScript SDK; here a plugin, 722 one-request sessions). The first records of a file (`queue-operation`, `session-owner`) name none. `sdk-py` was not seen and is assumed by analogy.
- A generated title is `{"type":"ai-title","aiTitle":…}`. A title the user gave is `{"type":"custom-title","customTitle":…}`. Both can appear many times; the last one counts, and a custom title wins over any generated one.

### How the adapter reads them

- **Listing.** The listing reads only the files' metadata, then describes each file from at most its first and its last 256 KiB, complete lines only: its folder (`cwd`), its title (the last custom title, else the last generated one, else the first message) and how it was started (the first `entrypoint`). A description is kept and asked again only when the file's modification time or length changed. A session whose first `entrypoint` is `sdk-ts` or `sdk-py` was made by a program and is left out of the listing; it is still found by its id. A listing can ask only for files changed since a time; the app asks for the last 30 days first and for all of them after. A projects folder, or a project's folder, that cannot be read for a reason other than not existing makes the listing fail, so the app keeps what it knows; a projects folder that does not exist yet means no sessions.
- **History.** A history is read from the end of the file: at most the last 16 MiB, at most 2,000 items, along the chain of replies that leads to the last message. When more is stored than is shown, the history says so, and the app shows "Earlier messages are not shown." On a hand-made file of 261 MB, reading the end took about 60 ms in a release build and about 580 ms in a debug build.
- **Last change.** The time a session last changed is its file's modification time; a file that is gone has none.

What reading them taught:

- `"uuid"` (with its quotes) is a good word to look for before parsing a line: it never matches inside `"parentUuid"` or `"logicalParentUuid"` (capital U), so a line that has it almost always has a top-level `uuid` key. `permission-mode` records have no `uuid` and need their own word.
- When reading from the middle of a file, the first, partial line must be dropped. A walk along the chain that then ends at a record with no parent cannot know whether that record was the file's first, so the history says that earlier messages are not shown. The same holds for a cut region that holds no message at all.
- The walk guards against links that run in a circle, which a broken or hand-edited file could hold. It costs one flag per record.
- Parsing only the top-level link fields of every record, and fully parsing only the records on the chain, keeps a 16 MiB region at about 60 ms in a release build.

### Known limits

- **Titles are looked for only at the start and the end of a large file.** A file over 512 KiB is described from its first and last 256 KiB. A title the user gave in the middle of such a file is not seen, so a later generated title, or the first message, names the session in the list instead.
- **A file that names no folder in its first or last 256 KiB is not listed.** It has no group to sit in. A file that names its folder there and cannot be read further on is listed, and opening it says why it could not be read.
- **A region that starts exactly at the start of a line loses that line.** The history reader drops everything up to the first line break of a region that does not start at the file's start, even when the cut fell on a line's start. The listing's reader reads one byte before its region to avoid this; the history reader does not.
- **A record with the same top-level link key twice** (`uuid`, `parentUuid` and the like) is skipped as broken. Not seen in the store.
- **The same session id in two project folders** gives whichever file is found first.
- **A rewrite of a file within the same second that keeps its exact length** would not be described again. Session files only grow by appending, so this does not happen in practice.

## Read in the program for plan 5

Added by plan 5 (the effort level, the right panel and Remote Control) on 2026-11-16. No agent ran `claude` for it. Its facts were read in the program's own text, version `2.1.323`, mostly in the descriptions of its own schemas; the recordings above come from `2.1.319`. Each point is marked:

- **Seen**: in a fixture or a raw recording above, or in the store on disk.
- **Read in the program**: read in its text, never seen working.
- **Guess**: inferred, not read.

Lines below that start with `> ` are what the adapter sends; the others are what the program would send. All of them are written by hand from the program's text, with made-up values and some keys left out: none is a recording. Plan 5's live test prints a line for most points, and each point says which line settles it. Until Jerod has run the live test, every point marked "read in the program" is open. Jerod ran it on 2026-11-16: `get_settings`, `apply_flag_settings` with `effortLevel` (both ways), `get_context_usage` at a session's start and after a turn, Remote Control on and off after a turn and before a first turn, and `--effort` on resume were all seen working. The refusal kinds and the states `reconnecting` and `policy_disabled` were not met and stay as read.

### Effort level

- **Seen.** The `initialize` reply's `models[]` entries carry `supportsEffort` and `supportedEffortLevels` (V9 shows one). **Read in the program:** both keys are there only for a model that takes levels, the levels are some of `low`, `medium`, `high`, `xhigh` and `max` in that order, and no entry names a default level. The adapter takes `supportedEffortLevels` as the model's levels only when `supportsEffort` is `true`.
- **Seen.** The `system`/`init` records of `2.1.319` have no `effort` key, only `per_turn_effort_active: true`. **Read in the program:** `init` has an optional `effort`, described as present on Remote Control's own frames. The adapter does not read it.
- **Read in the program.** A level at start: `--effort <level>`. An unknown value is ignored with a warning on stderr. The adapter passes it whenever the session has a level, also with `--resume`: nothing was found in the program that restores a level on resume.
- **Read in the program.** A change in a running session:

  ```json
  > {"type":"control_request","request_id":"rc_7","request":{"subtype":"apply_flag_settings","settings":{"effortLevel":"high"}}}
  {"type":"control_response","response":{"subtype":"success","request_id":"rc_7"}}
  ```

  The reply is a bare success, also for a value the program ignored. `max` is taken for the session only. A level the model cannot do is lowered to `high` when it is used. `apply_flag_settings` is in the set of requests the program handles while a turn runs; **guess:** so it can be sent during a turn and holds from the next request. Settled by: `live: effort changed to <level> in N ms`, for the change and for going back. A change sent during a turn stays unseen: the live test changes the level between turns.
- **Read in the program.** Reading the level in use:

  ```json
  > {"type":"control_request","request_id":"rc_8","request":{"subtype":"get_settings"}}
  {"type":"control_response","response":{"subtype":"success","request_id":"rc_8","response":{"applied":{"model":"claude-haiku-5-5","effort":"high"}}}}
  ```

  The reply also has `effective` and `sources`, left out here. `applied.effort` is a level or `null`; the program describes `applied` as what the next message will use. The adapter asks after the session starts, after a `set_model` reply, after every `result` and after every `apply_flag_settings` reply. It reads only `applied.effort` and keeps nothing else of the reply. When a level was asked for and another is read, it says "Claude Code goes on with <level>". Only the reading that follows the latest choice says it: when two choices come in quick succession the program may take both before the first reading, which then finds the second level; a reading after an older choice gives the level it finds and no failure (since the fix wave after plan 5's review). Settled by: `live: effort at start: <level>`.
- **Read in the program.** Which level a turn uses, the first that is set: `CLAUDE_CODE_EFFORT_LEVEL`, a level for that turn, the session's level (from the flag or from `apply_flag_settings`), the model's default. A `maxEffortLevel` in the settings caps any of them. Medium certainty on the order of the middle two. This is why the adapter reads the level back.
- **Never sent:** `update_settings`. It writes the user's settings file.
- **Seen in the store** (2026-11-15, the key shapes of one stored session, 2,455 `assistant` records): each `assistant` record has a top-level string `effort`, none under `message`. See "Stored sessions".

### Context and cost

- **Seen.** `assistant.message.usage` has `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens` and `output_tokens`. `result.modelUsage` maps each model to, among other keys, its `contextWindow` and `costUSD`. `result.total_cost_usd` is the session's running total at each finished turn.
- **Read in the program.** Claude Code's own share of the context window, as its status line shows it: `input_tokens + cache_creation_input_tokens + cache_read_input_tokens` of the last message that has usage, over the window of the model in use, at most 100%. Output tokens are not counted. Since plan 5 the adapter counts the same way, over the `contextWindow` that `modelUsage` gives for the model of that message, else over the largest window named. Since the fix wave after plan 5's review it takes the reading only from a top-level `assistant` record that has `usage` and whose model is not `<synthetic>`, the name the program gives a record it makes itself (as for a stopped turn), with a usage of nothing; any other record leaves the last reading as it is. Before that wave a record without usage cleared the reading, so no share came with the turn's end, and a synthetic one gave 0%. Neither record was seen live; the tests write them by hand. **Guess:** that window is the one the program means. Before plan 5 the adapter added the output tokens and divided by the largest window. Settled in part by: `live: context <N>% and cost known: yes`, which shows that a share and a cost arrive with a turn's end. Whether the share equals the program's own is not printed.
- **Read in the program.** A resumed session's total goes on from its stored total: the program writes a `cost-state` record and restores it on resume. For a fork this is not known.

### Remote Control

- **Seen** (V1): `ready`, the reply with `session_url`, then `connected`. Turning it off gives only the reply.
- **Read in the program.** All the values of `bridge_state` found, and what the adapter gives for each:
  - `ready`: starting.
  - `connected`: on, with the address the reply gave. After a drop it carries `recovered: true`, which the adapter does not read.
  - `reconnecting`: the connection dropped and is tried again. The adapter gives "reconnecting" and keeps the address.
  - `failed`, with `failure_kind` `auth`, `terminal`, `ended_elsewhere` or `network` (`terminal` when none is given): a failure. The adapter takes `auth` as a sign-in problem, `ended_elsewhere` as ended from another place, and the rest as other.
  - `policy_disabled`: refused by an organisation's policy, at start or later in the session. A failure of its own.
  - Any other value gives nothing. **Guess:** there may be more values; no list of them was found in the program.

  Its shape, from the program's text:

  ```json
  {"type":"system","subtype":"bridge_state","state":"failed","detail":"/login","failure_kind":"auth","bridge_epoch":1,"session_id":"00000000-0000-4000-8000-000000000002"}
  ```

- **Read in the program.** A refused request to turn it on:

  ```json
  {"type":"control_response","response":{"subtype":"error","request_id":"rc_9","error":"A made-up refusal","error_code":"remote_control_ended_elsewhere"}}
  ```

  `error_code` is `remote_control_auth`, `remote_control_terminal`, `remote_control_ended_elsewhere` or `remote_control_policy_disabled`, or it is missing (a network failure, or a policy not known yet). The adapter takes the codes as it takes the states. A `bridge_state` record usually comes before the error reply, so one refusal arrives twice; the core shows it once. A refused request to turn it off is a failed command, and Remote Control stays on. **Seen, not recorded:** a `failed` state and `remote_control_auth` on 2026-11-10, with only the token sign-in (V1). Settled in part by: `live: remote control: refused (<why>)`, should a refusal come; the live test listens 2 seconds more after the first refusal and prints every kind it heard, in order, as in `refused (Other, then Login)`. Which copy comes first decides the words the app shows (the reply's code and the state's `failure_kind` are read by different fields), so that line settles the mapping.
- **Read in the program.** For a sign-in problem `detail` is the bare text `/login`, not a sentence. The app never shows it: it says "Remote Control needs a full claude.ai sign-in. Run `claude auth login` in a terminal, then turn it on again."
- **Read in the program.** Turning it on needs no turn first: the program starts Remote Control with the messages it has, none included. The app asks for a session that wants it as soon as its program has started, before the first message. Settled by: `live: remote control before a first turn: on in N ms`. If that line says otherwise, a new session must ask when its first turn has ended.
- **Read in the program.** A drop the host did not ask for gives `reconnecting`, then `connected` with `recovered: true`, or `failed`. A change of policy gives `policy_disabled`; a change of sign-in gives `failed` with `terminal`. After `failed`, the next request to turn it on starts again. The live test does not settle these: they need a dropped connection or a policy.
- **Read in the program.** A prompt typed on another device goes on the program's own queue and runs as any turn; **guess:** the host sees the usual `init`, stream events, `assistant` and `result`. No `user` record for that prompt reaches stdout unless the program was started with `--replay-user-messages`. With it, every turn's prompt is echoed, the host's own included, as a `user` record with `isReplay: true`, and no field was found that says for certain where a prompt came from. The research lists an optional `origin` key on the echoed record, its meaning not established: whether the recording has it, and what it holds for a prompt typed here and for one typed on another device, is one of the things Task 13 reads from it. So the window shows the reply without its prompt, and Task 13 waits for a recording (`docs/notes-for-later-plans.md`, "The check with a second device").
- **Read in the program.** A permission request answered on another device ends with a `control_cancel_request` for it, the record of V7, which the adapter already gives as a withdrawn request.
- **Read in the program.** The flag `--remote-control` and the setting `remoteControlAtStartup` exist. Whether either acts in `-p` stream-json mode was not established. The app sends `remote_control` itself.

### Branch and files changed

- **Seen.** Nothing on stdout names the git branch: no `gitBranch` in any fixture or recording, and `init` has no branch. Stored records carry `gitBranch`, which the app does not read. The app reads the branch from the repository's own files (`.git`, or a worktree's `.git` file with its `gitdir:` line, then `HEAD`), never by running git.
- **Read in the program.** No request gives what this session changed, file by file. `get_workspace_diff` is the working tree's diff against a base, and is internal; `rewind_files` with `dry_run` needs file checkpointing and gives totals only. So the app counts the session's own Edit and Write calls that succeeded, from their inputs, as their rows in the conversation do. The program counts its own totals from the hunks of each result's `structuredPatch`, so the two can differ a little: for an edit with `replace_all` over many places, and for a file overwritten by Write.

## The context breakdown

Added on 2026-11-16, after plan 5, for the rows under "Context" in the panel. Marked as in "Read in the program for plan 5". No agent ran `claude` for it.

- **Seen** (V9). The request works:

  ```json
  > {"type":"control_request","request_id":"p2","request":{"subtype":"get_context_usage"}}
  ```

  Its reply (line 14 of `v9-control.jsonl`, whose `categories` the scrubber cut to three rows) has `categories`, a list of rows such as `{"id":"system_tools","name":"System tools","tokens":5870,"color":"inactive","kind":"used"}`, and `maxTokens`, the window. Only those two keys are read, and of a row only `name`, `tokens` and `kind`. Not read: `memoryFiles`, `mcpTools`, `skills` and `agents` (they hold paths and names), `gridRows`, `percentage`, `totalTokens` and the rest.
- **Seen.** Rows of the kind `used`. **Read in the program:** `kind` is also `free` (one row, "Free space", the rest of the window), `buffer` (the room kept for compacting, "Autocompact buffer" or "Compact buffer") or `deferred` (tool descriptions kept out of the window, listed for awareness and left out of the program's sums). The rows come in this order of ids: `system_prompt`, `system_tools`, `mcp_tools`, `mcp_instructions`, `mcp_tools_deferred`, `system_tools_deferred`, `custom_agents`, `memory_files`, `skills`, `messages`, then the buffer, then `free_space`; a used row with no tokens is left out by the program.
- **What the adapter does.** It asks after the session starts and after every `result`, whether the turn ended well or not, in the same list as the reading of the effort level and after it. From a success reply with `categories` and a `maxTokens` above nothing it gives the `used`, `buffer` and `free` rows in the program's order; a `deferred` row, a row of a kind it does not know and a row without a `name` or whole `tokens` are dropped. A refusal, a reply without the two keys, or one with no row left gives nothing, and nothing in the conversation. The panel shows each row with tokens; before a session's first turn here, its share over the bar is the used rows over the window.
- **Not known.** Whether the program answers before the first turn, when nothing has been sent yet: V9 asked mid-session. Settled by the live test's two lines, `live: context breakdown at start: …` and `live: context breakdown after a turn: …`, each a count of parts and their kinds, or `none`. Whether `free` and `buffer` rows come as read is settled by the kinds they print.

## Attachments, as sent

Added by plan 6 (attachments) on 2026-11-16. No agent ran `claude` for it. The lines below are what the adapter writes (`crates/claude/src/encode.rs`, `user_message`), written by hand with made-up values: none is a recording. The base64 is cut with `…`; the adapter writes it whole, on the one line. The keys are shown in a readable order; the adapter writes them sorted by name. Each point is marked as in "Read in the program for plan 5", and says which line of the live test settles it. Until Jerod has run the live test, every point not marked "seen" is open. Jerod ran it on 2026-11-16: a PNG image block sent through the app's own path was answered, a path under `Attached files:` made Claude read the file, the stored message read back as 1 image and 1 file with the text kept whole, and a line of about 5.2 MB went through. JPEG, GIF and WebP blocks stay unseen.

A message with text, one image and two files is one `user` line. Its content is one text block, then one `image` block per image with a base64 source. Files are not blocks: the text names them by path after a blank line and `Attached files:`, one `- PATH` line each.

```json
> {"type":"user","message":{"role":"user","content":[{"type":"text","text":"Does the sketch follow the plan?\n\nAttached files:\n- /home/alex/sketches/plan.md\n- /home/alex/sketches/shapes.txt"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgoAAAANSUhEUgAAABAAAAAQ…"}}]}}
```

A message of an image alone has no text block at all, not an empty one:

```json
> {"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo…"}}]}}
```

A message of files alone is a text block that starts with the heading:

```json
> {"type":"user","message":{"role":"user","content":[{"type":"text","text":"Attached files:\n- /home/alex/project/notes/login.md"}]}}
```

The `media_type` is `image/png`, `image/jpeg`, `image/gif` or `image/webp`, from the image's first bytes, not its name. An image made smaller by the app is sent as PNG when a pixel of it is not opaque, or when it is opaque and its PNG fits the limit; else as JPEG. A JPEG whose orientation tag says it is not upright is always turned and sent as a new JPEG. A path is written as it is, never quoted; a path that is not UTF-8 or holds a line break is never sent (its chip says "Its name cannot be sent").

What each shape rests on:

- **Seen** (V4). A text block followed by a PNG `image` block with a base64 source is answered, and the answer was about the image. The live test's `live: the answer names the colour: yes` sees it again through the app's own path: the chip read on the library thread, the line queued to the writer thread.
- **Read in the Messages format, never seen.** JPEG, GIF and WebP image blocks. Nothing in the live test sends one; Jerod's look does, by attaching one.
- **Never seen.** That a path under `Attached files:` makes Claude read the file with its own tools. Settled by `live: the answer names the file's word: yes|no`: the file the test writes starts with a word the answer can only name if the file was read. A `no` beside a `yes` for the colour means the path alone does not make Claude read it.
- **Never seen.** How the program stores such a message. The history reader (`crates/claude/src/history.rs`) counts the `image` blocks of a stored `user` record and takes the `Attached files:` block off the end of its text, giving the files back; it assumes the program stores the text block first and unchanged. Settled by `live: the stored message shows N image(s) and N file(s), text kept whole: yes|no`, read from the session's stored history through the backend: `1 image(s) and 1 file(s), text kept whole: yes` means the reader holds. Fewer images, no file, or `no` means the stored form differs, and the reader is corrected from what the test prints before plan 6 is merged. The line before it, `live: the stored history holds N message(s) of the user's`, says whether the program split the message into more than one record.
- **Read in the program, never seen.** The program's own limits for an image: 2,000 pixels a side and 5,242,880 bytes of base64. The app keeps under them by itself: an image is sent as it is only while it is within 2,000 pixels a side and 3,932,160 bytes, else made smaller. Nothing is sent past them.
- **Never seen.** A line of several megabytes on the program's input. Settled by `live: a 3 MB line went through: yes|no`: a PNG of noise of about 3.9 MB, a little under the app's limit, sent byte for byte as a line of about 5.2 MB of base64. `yes` means the turn it was sent in was answered. `no` means the turn failed, the program ended, or nothing came within 120 seconds, and the lines before it say which.

## Background tasks

Added by plan 7 (background running) on 2026-11-17. No agent ran `claude` for it. Plan 7 stops a program that has been idle a long while, and must not stop one that still runs a dev server or a build for the user; so the adapter reads the program's record of its background tasks.

The record, as the program's own text describes it (read in the program, version 2.1.323), written here by hand with made-up values:

```json
{"type":"system","subtype":"background_tasks_changed","tasks":[{"task_id":"t1","task_type":"local_bash","description":"npm run dev"},{"task_id":"t2","task_type":"local_agent","description":"review","off_host":true}],"uuid":"u","session_id":"s"}
```

- **Seen** (V1). The record arrives: twice in `v1-remote.jsonl`, each time with `tasks` an empty list. A record with a task in it was never seen; the shape of an entry is only what the program's text says.
- **Read in the program.** `tasks` is every live background task after the change, so each record replaces the last. The program sends nothing at start: no record means no task.
- **What the adapter does.** `crates/claude/src/wire.rs` reads the record as `Incoming::BackgroundTasks` and keeps its `tasks` whole; `crates/claude/src/translate.rs` (`background_tasks`) gives `SessionEvent::BackgroundTasks { running }`. An entry counts unless its `off_host` is `true`: a task off this computer does not need the program to stay up. A record whose `tasks` is missing or not a list gives `running: 1`, and an entry that is not an object counts as one, so a program is never stopped because its record could not be read. The record is no longer one of the lines the adapter notes on stderr as ignored.
- **A second guard, from the tool calls.** The record with a task in it was never seen, and it is the only thing between an idle program and a build or a dev server that still runs. So the adapter also counts from what it does see: a tool call whose input has `run_in_background` set to `true` (the field the program's shell and agent tools take for work that outlives the call; written from the program's text, never seen live). From such a call on, `translate.rs` (`background_started`) gives `SessionEvent::BackgroundTasks` with at least one task running, at once and whatever the records said before. A record that arrives after the call takes over, and from then on the records decide as above. If records never come, or a later version of the program changes them, the count stays at one or more until the program ends: a program that started a background task is then never stopped for idleness. That is a program too many kept up, never a build ended. A call a sub-agent makes counts too, although the window shows nothing of it. A tool that asks the user (`AskUserQuestion`, `ExitPlanMode`) never counts. The guard lives in the translator, which serves one program: the next program starts with nothing counted. Nothing changes in the seam between the adapter and the app.
- **In the session.** `Session.background` holds the last count; it is 0 at `Started` and at `Exited`. Nothing in the window shows it.
- **What the live test settles.** Three points were never seen: that an entry marks a task off this computer with the key `off_host`, that `tasks` lists only the tasks still running, and that a record comes whenever a background task starts. The first two fail safe: a key under another name counts the task, and a finished task left in the list keeps the program up. The third does not, by the record alone: if no record comes when a task starts, only the guard above keeps the program up, and the guard rests on the tool call carrying `run_in_background`, which was never seen either. So the live test starts one background task. In its session for attachments it asks the program to run `sleep 20` in the background and answers the permission request itself, through the app, allowing only the shell tool with `sleep` and a number as its whole command. It prints `live: a background task record that counted at least one task arrived: yes|no` and `live: the adapter's own guard counted the tool call: yes|no`. The app is given one count for both, so the test tells them apart by their order alone: a count directly after the shell call is the guard's, and a count on its own is the record's. A record that arrives right after a call the guard did not count, or the guard's count for a sub-agent's call, can so be put on the wrong line; both mistakes err towards a task running. When the program asks for anything else, the request is denied and the first line reads `not tried (the program asked for something else)`. It also prints `live: background task records that arrived: N`, counted over both its runs and all its programs (the guard's count among them), and, when there was one, `live: the last one counted M task(s)`. Until one of the two lines has said `yes`, the point stays open, for Jerod's look as well: ask a session to run `sleep 600` in the background, leave it for longer than `MINDRUNE_IDLE_SECONDS`, and see that its `claude` process is still there. Never tried at all: a wake-up the program schedules for itself, and whether that is among its tasks.

## A stored image, read again

Added by plan 7 (opening a picture) on 2026-11-17. No agent ran `claude` for it, and nothing below was seen working.

A picture sent in this run opens from memory: the app keeps the image it sent. A picture of a session opened again from the history is read again from the store, by the message it was sent in:

- **The message's id.** Each record of a stored session has its own `uuid`. The history reader (`crates/claude/src/history.rs`) keeps the `uuid` of each `user` record as that message's id (`HistoryItem::User.stored`), and the app keeps it with the message's pictures. The reader already follows `uuid` links between records, so the key is known to be there; that it finds a message with its images again was never seen.
- **The read.** `Agent::read_image(id, message, index)` (`read_image` in `history.rs`) reads the whole session file, not only the last 16 MiB the history reader reads, line by line, and takes the first record whose `uuid` is `message`. Its `index`-th image block, decoded from base64, is the image; the message's other images are not decoded. It says "the session is not in the store", "the message is not in the session" or "the message has no such image" when it cannot give one. The window then keeps the small picture, with the line "The full picture cannot be read."
- **A copy.** A session continued as a copy ("Continue as a copy") asks for its stored pictures under the id its conversation was read from: the original's, while it holds the conversation it was made from, and its own once that conversation has been given back to the store and read again from the copy's file. Whether the copy's file keeps the original messages' `uuid`s is not known. If it does not, a picture read under the copy's own id shows small with the line, never as another picture.
- **What the live test settles.** After the session sent attachments has its stored history read back, the test prints `live: the stored message has an id: yes|no` and, with an id, reads the message's image again by it through the backend, as a press on a picture of a reopened session does: `live: its image read again by that id: N bytes, the same as were sent: yes|no`. `yes` settles the read. `no` with a byte count means the store keeps another image than was sent; `not read`, with the reader's words, means the id or the read is wrong, and the reader is corrected from what the test prints. The copy is not tried by the test.

## A session's files, and removing them

Added by plan 8 (the Deleted view) on 2026-11-17, for `claude` 2.1.323. No agent ran `claude` or removed anything from a real store for it.

**Where.** Claude Code's folder is `$CLAUDE_CONFIG_DIR` when that variable is set and not empty, else `~/.claude` (`root_from` in `crates/claude/src/remove.rs`). Only an absolute folder counts: with a relative `CLAUDE_CONFIG_DIR`, or no home folder, the app has no store. The adapter lists, reads and removes there, and nowhere else.

**A store that is not there is an error, never an empty store.** The app takes a complete listing for everything there is, and `Missing` for "this session is gone": after either it forgets the deletion, the kept name and the group place of a session it no longer finds. So neither is ever answered for a store that was not looked through. Listing (`list_sessions`, `list_entries` in `history.rs`) answers the error "the store could not be found" when the folder is not known, when it does not exist (a home folder not mounted, a `CLAUDE_CONFIG_DIR` naming a folder not made yet), and when `projects` in it is a link whose target does not exist. A folder that exists with no `projects` in it yet lists as empty: that is a fresh install. An empty mount point looks the same, so in the app a complete listing that holds no session at all forgets nothing: a deleted session's file stays in the store for its week, and a store with no session in it was not the one the deletions were made in. Removing (`remove`, and `paths_of`) answers the same error in the first two cases and whenever there is no `projects` to look through, the fresh install included, and touches nothing then, not the side folders either. In the app a failed listing keeps every deleted mark, every kept name and every group place, and a failed removal leaves the session deleted, to be tried again an hour later. What this cannot guard against is a start with another `CLAUDE_CONFIG_DIR` that names a real store: its complete listing does not hold the first store's sessions, so their deletions, names and places are forgotten. Before plan 8 it always read `~/.claude/projects`, while it passed `CLAUDE_CONFIG_DIR` on to the program.

**The four kinds of path.** One session leaves at most these, each named by its id:

- `projects/<project>/<id>.jsonl`, the transcript. It is looked for in every project folder, as reading does; normally there is one.
- `projects/<project>/<id>/`, a folder beside a transcript, in some sessions only: sub-agents' transcripts, long tool results, a title file.
- `file-history/<id>/`, the copies of files the session edited.
- `session-env/<id>/`, empty wherever it was seen.

The names were read in one real store, the one of the machine this was built on, by name and kind only, never by content. Nothing else was read: not the program's source and not its documentation. `tasks/<id>/` and `todos/<id>-agent-*.json` were not there in this version, and nothing removes them.

**Shared, and never touched.** `history.jsonl` (its lines about the session stay), `.claude.json` (in the folder and in the home folder), `sessions/`, `shell-snapshots/`, `jobs/`, `daemon/`, `backups/`, `settings.json` and the credentials, and in each project folder `memory/`, `.session-aliases` and `<uuid>.desktop-released.json` (whose ids match no session's). The project folder itself stays, also when it is left empty. Whether any of these refers to a session by its id was not looked at. `claude purge` removes a whole project, never one session, and also edits `.claude.json`: the app does not use it.

**Removing.** `remove_session(id, unless_after_unix)` (`remove` in `crates/claude/src/remove.rs`) is the only code that changes anything in Claude Code's folder.

- It takes only an id in the written form of a UUID: 36 characters, 8, 4, 4, 4 and 12 hexadecimal digits with dashes between. Any other id names no path, and its removal answers the error "the session could not be removed", never `Missing`: the listing does not limit ids to UUIDs, so a session with such an id would otherwise lose its deletion and come back.
- It removes only the paths above, those that exist. A real folder goes with all it holds; a file goes as itself; a link goes as a link, and what it points to stays, also for a link inside the session's folder.
- Links above the session's own paths are followed, as reading follows them: Claude Code's folder, `projects`, a project folder, `file-history` and `session-env` may each be a link (a dotfiles setup may link `~/.claude` itself), and the session is removed where they point.
- When a transcript was modified later than `unless_after_unix`, the session was used again since it was deleted: nothing is removed and the answer is `Changed`. With transcripts in two project folders, the newest decides. The times are read again after the side folders went and before the transcripts go; a change found then also gives `Changed`, and the transcripts stay.
- With no transcript in any project folder of a `projects` that was looked through, the answer is `Missing`, and the other kinds of path, left over, still go. Claude Code seems to clear old transcripts itself (all the oldest ones seen were about 30 days old), so a session can be gone before the app removes it.
- The transcripts go last, so a session that could not be removed whole is still listed. A path that cannot be removed ends the removal with the words "the session could not be removed", which name no path.
- A removal that fails partway, or finds a change at the second look, has already removed the session's side folders (its rewind copies in `file-history/<id>/` and its sub-agent folder beside the transcript), and its transcript stays.

**What the live test settles.** At its very end, after it has closed its programs, the live test removes through `remove_session` the three sessions it made in that run (its first session, the one opened to the side, and the one sent attachments), by their ids, and nothing else: no session it did not make and none whose id it does not know. It removes a session only once it has seen that session's program end: closing the app waits for the first two, and for the session sent attachments, which was waited for only up to a limit, the test waits again, up to twenty seconds, and keeps it with the line `live: a test session was still running; kept` if its program still runs. It passes as `unless_after_unix` the moment that wait ended plus ten seconds, as the app does for a session it deleted; since the removal follows at once, that value guards little here, and it is the wait that keeps a running program's files from being removed under it. It prints `live: the test's own sessions removed from the store: N of M`, and a line for each one not removed (not in the store, changed after the test closed it, or an error, by its kind only). Then it looks through the whole of Claude Code's folder for any path whose name holds the id of a session that is gone (removed, or not in the store under its id): by names alone, at most four levels deep and at most 200,000 names, opening no file and following no link (`leftovers` in `crates/app/tests/live.rs`). It prints `live: paths still named by a removed session's id: N` and, when N is not zero, `live: they sit under: …`, the first-level folders under Claude Code's folder that those paths sit under (`projects`, `file-history` and so on), never a deeper name, an id or a project folder's name; a path at the top whose own name holds an id is given as `(the top of the folder)`. It says so when the look stopped at its limit of names or could not list a folder, and it prints `live: folders not looked into, being links: N` (a count, no name) when a first-level folder is a link: such a folder is not looked into, so with `projects` linked elsewhere `0` says nothing of what is there. None of it fails the test.

A test that fails before its end tidies up as well. On every way out it removes those of its sessions whose programs it has seen end, with the same guard on the transcript's time, and prints `live: the test left early; its sessions removed from the store: N of M, kept because a program may still run: K`. It removes nothing else: a session whose program may still run is kept, because that program could write its files again after they were removed and leave a partial session in the store; a session whose id the test never learned cannot be named; and nothing the test did not make is its to remove.

`3 of 3` removed and `0` paths left settle that removing works on a real store for sessions the program wrote, transcripts and side folders alike, and that this version of the program keeps no other path named by a session's id within four levels of its folder. A count that is not zero, with a folder in `they sit under` that is not one of `projects`, `file-history` and `session-env`, shows a kind of path these notes do not name. What the look cannot see is an id written inside a file, such as a line of `history.jsonl`: the test reads no shared file, so whether a shared file refers to a removed session stays unknown. A `not in the store` line means the program stored that session under another id or not at all; the look then shows whether a path is named by the id all the same.

## The task list and a tool of the app's own

Seen live on 2026-11-19 with `claude` 2.1.323 and the model `claude-opus-5-5`, driven by `mindrune-probe raw`. Fixtures: `v12-tasks.jsonl` (two turns that keep a task list; the program was started with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` in its environment and with `--prompt-suggestions`) and `v14-choices.jsonl` (two turns; the probe was started with `--choices`, which serves the choice tool). Both are scrubbed and then trimmed as the earlier ones were: the long lists of a session's start are cut short and thinking signatures replaced. What the probe sent is in no fixture; the lines marked `> ` below are from the raw recordings.

### The task list

Seen:

- **The tools are there only when asked for.** With `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`, `init`'s `tools` holds `TaskCreate`, `TaskGet`, `TaskList` and `TaskUpdate` (line 2 of `v12-tasks.jsonl`). Without it, on this model, it holds none of them (`v14-choices.jsonl`, line 5): the documentation says the newer models keep no written list unless the session opts in.
- **They are deferred tools.** Before its first use Claude loads them with a `ToolSearch` call whose input is `{"query":"select:TaskCreate,TaskUpdate","max_results":2}` (line 17), an ordinary `tool_use` with an ordinary result.
- **A step is made with `TaskCreate`**: input `{"subject":"Create a.txt","description":"Write a.txt with one line","activeForm":"Creating a.txt"}` (line 34). Its id is not in the input. It comes with the result, on the `user` record that carries the `tool_result` block (line 38): the block's text is `Task #1 created successfully: Create a.txt`, and the record's top-level `tool_use_result` is `{"task":{"id":"1","subject":"Create a.txt","description":"Write a.txt with one line"}}`.
- **A step changes with `TaskUpdate`**: input `{"taskId":"1","status":"in_progress"}` (line 69), later `{"taskId":"1","status":"completed"}` (line 92). The result's text is `Updated task #1 status`, and `tool_use_result` is `{"success":true,"taskId":"1","updatedFields":["status"],"statusChange":{"from":"pending","to":"in_progress"}}`.
- **The ids are strings of numbers that count on through the session**: the first turn made 1 to 3, the second 4 to 6, and the finished steps of the first turn were never removed by the program.
- A `tool_result` block of a call that worked has no `is_error` key at all.
- With `--prompt-suggestions` the program sends, after a turn's `result`, `{"type":"prompt_suggestion","suggestion":"Now add a third line to each file.",…}` (line 412): one line of text, its guess at the user's next message. The app does not ask for it.

Read in the documentation, not seen: `TaskUpdate` also takes `subject`, `description` and `activeForm`, and the status `deleted`, which removes the step; the model may write `id` or `task_id` for `taskId` and `active_form` for `activeForm`, which the program repairs before it runs the tool but not in the record it sends; with `CLAUDE_CODE_ENABLE_TASKS=0` the program gives the older `TodoWrite` in place of these tools.

Guessed: that a stored transcript keeps the same result under `toolUseResult` on the `user` record. The store was not read for these notes; the adapter looks for both spellings and falls back to the result's text.

### A tool of the app's own

The program is given a server it reaches through the session's own channel: `--mcp-config {"mcpServers":{"mindrune":{"type":"sdk","name":"mindrune"}}}`. `--allowedTools mcp__mindrune__offer_choices` allows its one tool without a request, and `--append-system-prompt` adds the words on when to use it.

Seen, in this order (`v14-choices.jsonl`, with the probe's lines from the raw recording):

```
> {"type":"control_request","request_id":"probe_1","request":{"subtype":"initialize","hooks":null}}
{"type":"control_request","request_id":"…","request":{"subtype":"mcp_message","server_name":"mindrune","message":{"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"claude-code",…,"version":"2.1.323"}},"jsonrpc":"2.0","id":0}}}
> {"type":"control_response","response":{"subtype":"success","request_id":"…","response":{"mcp_response":{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"mindrune","version":"0.1.0"}}}}}}
{"type":"control_response",…}                                   the answer to the probe's own initialize
{…"subtype":"mcp_message","server_name":"mindrune","message":{"method":"notifications/initialized","jsonrpc":"2.0"}}
> {…"response":{"mcp_response":{"jsonrpc":"2.0","result":{}}}}
{…"subtype":"mcp_message","server_name":"mindrune","message":{"method":"tools/list","jsonrpc":"2.0","id":1}}
> {…"response":{"mcp_response":{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"offer_choices",…}]}}}}
```

- **The server's handshake comes after the host's `initialize` request and before its answer**, and before any message of the user's. Every `mcp_message` request wants a successful `control_response` whose `response` is `{"mcp_response": <the JSON-RPC answer>}`, a notification too (an empty result).
- `init` then lists the server as `{"name":"mindrune","status":"connected","source":"sdk"}` and the tool as `mcp__mindrune__offer_choices` (line 5).
- **The tool is deferred as well**: Claude first calls `ToolSearch` with `{"query":"select:mcp__mindrune__offer_choices","max_results":1}` (line 20).
- **The call**: an ordinary `tool_use` block named `mcp__mindrune__offer_choices` with the whole input (line 50), then the request to the server (line 53): `{"method":"tools/call","params":{"name":"offer_choices","arguments":{"question":"…","choices":[{"label":"Keep them","message":"Keep the three text files."},{"label":"Delete them","message":"Delete the three text files in this folder."}]},"_meta":{"claudecode/toolUseId":"toolu_…","progressToken":2}},"jsonrpc":"2.0","id":2}`. The server's answer `{"content":[{"type":"text","text":"Shown to the user."}]}` comes back to Claude as the call's `tool_result` (line 55), whose `tool_use_result` is the same list of content.
- **No `can_use_tool` request came for it**: `--allowedTools` settled it (`tool_result_meta` says `"permission_decision":{"decision":"accept","source":"config","reason_type":"rule"}`).
- **The turn goes on after the call.** Claude wrote a closing reply after it (line 81) and the `result` came after that (line 85). The buttons are therefore offered before the turn has ended.
- Claude did not repeat the question in its reply, as the instruction of that recording asked. The app's instruction now asks the reply to make sense without the buttons and to end by naming the decision in a few words.
- A new kind of record appears while Claude thinks: `{"type":"system","subtype":"thinking_tokens","estimated_tokens":…,"estimated_tokens_delta":…}` (lines 29 and 32). The app has no use for it.
- **A message sent while a turn runs is taken in at once**, not after the turn: in an earlier recording (not a fixture) a second message, sent while a permission request waited, changed what Claude did in that same turn. The app already holds a message back until the turn has ended.

Read in the documentation, not seen: a server's entry may say `"alwaysLoad": true`, and a tool may carry `"_meta":{"anthropic/alwaysLoad":true}` in the list, either of which loads the tool with the session's first prompt, so that no `ToolSearch` is needed; `alwaysLoad` makes the start wait for the server's tools, up to five seconds.

### What the live test settles

The live test (`crates/app/tests/live.rs`, run only by Jerod) asks its own session for a task list of two steps, "one" ticked off and "two" left open, and for two choices, and prints: `live: task steps added N, changed M, open at the turn's end K` (the list kept with the task tools turned on, and each step's id read from its result; K is expected to be 1), `live: tool lines of the turn: N (named ToolSearch: M)` (the hidden calls kept from the tool lines), `live: choices offered: true|false, buttons N, shown after the turn: true|false` (the app's server reached and its tool called), `live: the choice was sent and answered: true|false`, and, if the program ends during the step, `live: the program ended during the task step`. Once the step is over (after the choice's turn, or the step's own turn when no choice was sent) it prints `live: task steps at the end of the first run: N`: 2 while "two" is still open, 0 when Claude ticked it in the choice's turn and the list cleared. In its second run, from the stored conversation read back, it prints `live: stored history gives task steps N, an offer: true|false` and `live: hidden calls among the stored tool items: N`. N of the first is expected to equal the first run's `task steps at the end of the first run`; a smaller N means that steps' ids could not be read from the stored results, so the guess above about how a stored result names them failed, whichever field it is. The line does not show which field held the id. An offer of true shows that the stored conversation gives back an offer made after the user's last message there: the choice was never answered, or Claude offered again in the turn that answered it, which the test cannot tell apart; it does not by itself mean the store is read wrongly. The count of hidden calls shows only that the stored reader hides, by their names and inputs, what the live one hides. Whether `alwaysLoad` works is not settled by these lines: a `ToolSearch` that loads only the choice tool is hidden, so it is never counted among the tool lines, and `named ToolSearch: 0` with choices offered says only that the user saw no search.
