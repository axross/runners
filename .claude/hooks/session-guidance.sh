#!/bin/bash

set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" = "true" ]; then
  echo "Read this checkout's AGENTS.md and follow its Response Approach. Before planning, delegating or editing a change, load its prerequisites and Loop Engineering."
fi
