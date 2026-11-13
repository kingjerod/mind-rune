#!/usr/bin/env bash
# Stand-in for `claude auth status` in tests. $STUB_AUTH chooses what it prints:
#   full  - logged in with a saved full login, whatever the environment
#   token - logged in by token only when CLAUDE_CODE_OAUTH_TOKEN is set
#   none  - logged out
#   sleep - writes its pid to $STUB_PID_FILE, then sleeps far past any timeout
#   count - adds a line to $STUB_COUNT_FILE for each run, and is logged out
#   leak  - like full, but writes "leak" on stderr and exits 9 when
#           ANTHROPIC_CANARY is set: a parent session's variables reached it
case "$STUB_AUTH" in
  sleep)
    echo "$$" > "$STUB_PID_FILE"
    exec sleep 30
    ;;
  count)
    echo run >> "$STUB_COUNT_FILE"
    echo '{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}'
    ;;
  leak)
    if [ -n "${ANTHROPIC_CANARY+x}" ]; then
      echo "leak" >&2
      exit 9
    fi
    echo '{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty", "email": "user@example.com", "subscriptionType": "max"}'
    ;;
  full)
    echo '{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty", "email": "user@example.com", "subscriptionType": "max"}'
    ;;
  token)
    if [ -n "${CLAUDE_CODE_OAUTH_TOKEN+x}" ]; then
      echo '{"loggedIn": true, "authMethod": "oauth_token", "apiProvider": "firstParty"}'
    else
      echo '{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}'
    fi
    ;;
  *)
    echo '{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}'
    ;;
esac
exit 0
