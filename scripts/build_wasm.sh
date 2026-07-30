#!/bin/bash
# Regression check script for WASM build failures
# This script builds all deployable contracts to WASM and fails if any build fails

set -e

echo "Building all deployable contracts to WASM..."

# List of deployable contracts that must build successfully
DEPLOYABLE_CONTRACTS=(
    "deal_escrow"
    "staking_pool"
    "transaction-receipt-contract"
    "rent_wallet"
    "rent_payments"
    "staking_rewards"
    "mvp_staking_pool"
    "vesting_schedule"
    "whistleblower_rewards"
    "tenant_reputation"
    "inspector_bond"
    "oracle_price_feeds"
)

FAILED_CONTRACTS=()

for contract in "${DEPLOYABLE_CONTRACTS[@]}"; do
    echo "Building $contract..."
    if cargo build --release --target wasm32-unknown-unknown -p "$contract" > /dev/null 2>&1; then
        echo "✓ $contract built successfully"
    else
        echo "✗ $contract build FAILED"
        FAILED_CONTRACTS+=("$contract")
    fi
done

if [ ${#FAILED_CONTRACTS[@]} -gt 0 ]; then
    echo ""
    echo "ERROR: The following contracts failed to build to WASM:"
    for contract in "${FAILED_CONTRACTS[@]}"; do
        echo "  - $contract"
    done
    exit 1
else
    echo ""
    echo "SUCCESS: All deployable contracts built successfully to WASM"
    exit 0
fi
