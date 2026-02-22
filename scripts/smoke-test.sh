#!/bin/bash
# CrabClaw smoke test — runs end-to-end against the real (or mock) LLM API.
# Usage: ./scripts/smoke-test.sh
#
# This script verifies the full pipeline:
#   1. cargo build passes
#   2. All unit + integration tests pass
#   3. CLI `run --prompt` returns a non-empty reply from the configured API
#
# Exit code 0 = all good, non-zero = something broke.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

cd "$(dirname "$0")/.."

echo "=== CrabClaw Smoke Test ==="
echo ""

# Step 1: Build
echo -n "1. Building... "
if cargo build --quiet 2>&1; then
    echo -e "${GREEN}OK${NC}"
else
    echo -e "${RED}FAILED${NC}"
    exit 1
fi

# Step 2: Clippy
echo -n "2. Clippy... "
if cargo clippy --all-targets --all-features --quiet -- -D warnings 2>&1; then
    echo -e "${GREEN}OK${NC}"
else
    echo -e "${RED}FAILED${NC}"
    exit 1
fi

# Step 3: Tests
echo -n "3. Running tests... "
TEST_OUTPUT=$(cargo test 2>&1)
PASS_COUNT=$(echo "$TEST_OUTPUT" | grep "^test result:" | grep -oE "[0-9]+ passed" | awk '{s+=$1} END{print s}')
FAIL_COUNT=$(echo "$TEST_OUTPUT" | grep "^test result:" | grep -oE "[0-9]+ failed" | awk '{s+=$1} END{print s}')

if [ "${FAIL_COUNT:-0}" -eq 0 ]; then
    echo -e "${GREEN}OK${NC} (${PASS_COUNT} passed)"
else
    echo -e "${RED}FAILED${NC} (${FAIL_COUNT} failed)"
    echo "$TEST_OUTPUT"
    exit 1
fi

# Step 4: Live API smoke test (only if API key is configured)
if [ -f .env.local ]; then
    echo -n "4. Live API call... "
    REPLY=$(cargo run --quiet -- run --prompt "Reply with exactly: SMOKE_TEST_OK" 2>&1 || true)

    if echo "$REPLY" | grep -qi "SMOKE_TEST_OK"; then
        echo -e "${GREEN}OK${NC}"
    elif echo "$REPLY" | grep -qi "error"; then
        echo -e "${RED}FAILED${NC}"
        echo "   API returned error: $REPLY"
        exit 1
    else
        # Model replied but didn't follow instructions exactly — still OK
        echo -e "${YELLOW}OK (reply received but content varied)${NC}"
        echo "   Reply: $(echo "$REPLY" | head -1)"
    fi
else
    echo -e "4. Live API call... ${YELLOW}SKIPPED${NC} (no .env.local)"
fi

echo ""
echo -e "${GREEN}=== All checks passed ===${NC}"
