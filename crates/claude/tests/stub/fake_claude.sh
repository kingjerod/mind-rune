#!/usr/bin/env bash
# Stand-in for the `claude` program in tests of ClaudeAgent. Ignores its
# arguments and plays a canned conversation, reading stdin line by line.
#   "subtype":"initialize"     -> a success reply listing two models
#   please-die                 -> "stub died" on stderr, exit 7 (checked first)
#   ignore-eof                 -> writes its pid to stub.pid in its working directory,
#                                 then runs on, ignoring input and its end, until killed
#   need-permission            -> a turn that asks to run Bash (request req_p); it
#                                 finishes only when an allow reply for req_p arrives
#   "subtype":"set_model"      -> a success reply naming claude-sonnet-5-5
#   "subtype":"get_usage"      -> a success reply with the two rate-limit windows
#   "subtype":"generate_session_title"
#                              -> the title "Listing the folder" announced, then the same
#                                 title in the success reply
#   "type":"user"              -> a canned turn up to a hook_callback request; the turn's
#                                 result is withheld until the request is answered
#   "subtype":"remote_control" -> a refusal that asks for a login
#   "subtype":"error" + req_h  -> the withheld result: proves the unsupported reply arrived
#   end of input               -> exit 0
# Started with ANTHROPIC_CANARY set, it writes "leak" on stderr and exits 9:
# the parent session's variables reached it.

request_id() {
  printf '%s\n' "$1" | sed -n 's/.*"request_id":"\([^"]*\)".*/\1/p'
}

MODELS='[{"value":"default","displayName":"Default","resolvedModel":"claude-haiku-5-5"},{"value":"sonnet","displayName":"Sonnet","resolvedModel":"claude-sonnet-5-5"}]'
INIT='{"type":"system","subtype":"init","session_id":"s1","model":"claude-haiku-5-5","permissionMode":"default","claude_code_version":"2.1.319","cwd":"/work"}'
MSG_START='{"type":"stream_event","event":{"type":"message_start","message":{}}}'
TEXT_START='{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}'
DELTA_OK='{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}}'
BLOCK_STOP='{"type":"stream_event","event":{"type":"content_block_stop","index":0}}'
ASSISTANT_OK='{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":40000,"cache_read_input_tokens":9000,"cache_creation_input_tokens":0,"output_tokens":1000}}}'
MSG_STOP='{"type":"stream_event","event":{"type":"message_stop"}}'
RATE_LIMIT='{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":0.08,"resetsAt":1794385200},"seven_day":{"utilization":0.06,"resetsAt":1794776400}}}}'
RESULT_OK='{"type":"result","is_error":false,"duration_ms":915,"total_cost_usd":0.002,"result":"ok","modelUsage":{"claude-haiku-5-5":{"contextWindow":1000000}}}'
TOOL_CALL='{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t_p","name":"Bash","input":{"command":"ls"}}]}}'
PERMISSION_REQUEST='{"type":"control_request","request_id":"req_p","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"},"tool_use_id":"t_p"}}'
TOOL_DONE='{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t_p","content":"a.txt"}]}}'
USAGE='{"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":37,"resets_at":"2026-11-11T08:19:59.618411+00:00"},"seven_day":{"utilization":10,"resets_at":"2026-11-15T20:59:59.618435+00:00"}}}'
HOOK_REQUEST='{"type":"control_request","request_id":"req_h","request":{"subtype":"hook_callback","callback_id":"cb_1","input":{}}}'

if [ -n "${ANTHROPIC_CANARY+x}" ]; then
  echo "leak" >&2
  exit 9
fi

while IFS= read -r line; do
  if [[ "$line" == *please-die* ]]; then
    echo "stub died" >&2
    exit 7
  elif [[ "$line" == *ignore-eof* ]]; then
    echo "$$" > stub.pid
    trap '' TERM HUP INT
    while :; do sleep 0.1; done
  elif [[ "$line" == *need-permission* ]]; then
    printf '%s\n' "$INIT" "$MSG_START" "$TOOL_CALL" "$MSG_STOP" "$PERMISSION_REQUEST"
  elif [[ "$line" == *'"type":"control_response"'* && "$line" == *req_p* && "$line" == *'"behavior":"allow"'* ]]; then
    printf '%s\n' "$TOOL_DONE" "$RESULT_OK"
  elif [[ "$line" == *'"subtype":"set_model"'* ]]; then
    id=$(request_id "$line")
    printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"model":"claude-sonnet-5-5"}}}\n' "$id"
  elif [[ "$line" == *'"subtype":"generate_session_title"'* ]]; then
    id=$(request_id "$line")
    printf '%s\n' '{"type":"system","subtype":"session_title_changed","title":"Listing the folder","session_id":"s1"}'
    printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"title":"Listing the folder"}}}\n' "$id"
  elif [[ "$line" == *'"subtype":"get_usage"'* ]]; then
    id=$(request_id "$line")
    printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":%s}}\n' "$id" "$USAGE"
  elif [[ "$line" == *'"subtype":"initialize"'* ]]; then
    id=$(request_id "$line")
    printf '{"type":"control_response","response":{"subtype":"success","request_id":"%s","response":{"commands":[],"models":%s,"claude_code_version":"2.1.319","current_permission_mode":"default","remote_control_available":true}}}\n' "$id" "$MODELS"
  elif [[ "$line" == *'"type":"user"'* ]]; then
    printf '%s\n' "$INIT" "$MSG_START" "$TEXT_START" "$DELTA_OK" "$ASSISTANT_OK" "$BLOCK_STOP" "$MSG_STOP" "$RATE_LIMIT" "$HOOK_REQUEST"
  elif [[ "$line" == *'"subtype":"remote_control"'* ]]; then
    id=$(request_id "$line")
    printf '{"type":"control_response","response":{"subtype":"error","request_id":"%s","error":"Claude.ai login was rejected","error_code":"remote_control_auth"}}\n' "$id"
  elif [[ "$line" == *'"subtype":"error"'* && "$line" == *req_h* ]]; then
    printf '%s\n' "$RESULT_OK"
  fi
done
exit 0
