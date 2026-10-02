#!/usr/bin/env bash

# MagicalCryptoWallet CoinJoin Payment Runner
# Starts coinjoin and monitors payments, adapting to new/cancelled payments

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/rpc-common.sh"

# Check RPC connection
status=$(mcw_rpc '{"jsonrpc":"2.0","id":"1","method":"getstatus"}' 2>/dev/null)
if [ -z "$status" ]; then
    echo "Error: Cannot connect to Magical Crypto Wallet RPC at $RPC_ENDPOINT"
    echo "Make sure MagicalCryptoWallet is running and RPC is enabled in Config.json"
    exit 1
fi

mcw_wait_ready || exit 1
echo "Wallet ready."

# Handle Ctrl+C gracefully
cleanup() {
    echo ""
    echo "Stopping coinjoin..."
    mcw_rpc '{"jsonrpc":"2.0","id":"1","method":"stopcoinjoin"}' > /dev/null || exit 1
    echo "CoinJoin stopped."
    exit 0
}
trap cleanup SIGINT

get_pending() {
    mcw_rpc '{"jsonrpc":"2.0","id":"1","method":"listpaymentsincoinjoin"}' \
        | jq '[.result[] | select(.state[0].status == "Pending")] | sort_by(.address)'
}

show_pending() {
    local payments="$1"
    local count=$(echo "$payments" | jq 'length')
    if [ "$count" -eq 0 ]; then
        echo "  (none)"
    else
        echo "$payments" | jq -r '.[] | "  \(.amount) sats -> \(.address)"'
    fi
}

# Show initial state
echo ""
echo "=== Wallet: Magical Crypto Wallet ==="
echo ""
echo "Pending payments:"
prev=$(get_pending)
show_pending "$prev"
prev_count=$(echo "$prev" | jq 'length')

if [ "$prev_count" -eq 0 ]; then
    echo ""
    read -p "No pending payments. Start coinjoin anyway? [y/N]: " confirm
    if [[ "${confirm^^}" != "Y" ]]; then
        exit 0
    fi
fi

# Start coinjoin
echo ""
mcw_authorized_rpc startcoinjoin '[]' > /dev/null || exit 1
echo "=== CoinJoin started ==="
echo ""

# Track payments
prev_addrs=$(echo "$prev" | jq -r '.[].address' | sort)
ever_had_payments=false
if [ "$prev_count" -gt 0 ]; then
    ever_had_payments=true
fi

while true; do
    curr=$(get_pending)
    curr_count=$(echo "$curr" | jq 'length')
    curr_addrs=$(echo "$curr" | jq -r '.[].address' | sort)

    # Track if we ever had payments
    if [ "$curr_count" -gt 0 ]; then
        ever_had_payments=true
    fi

    # Check for completed payments
    if [ -n "$prev_addrs" ]; then
        for addr in $prev_addrs; do
            if ! echo "$curr_addrs" | grep -q "^${addr}$"; then
                amount=$(echo "$prev" | jq -r ".[] | select(.address == \"$addr\") | .amount")
                echo "[$(date +%H:%M:%S)] Sent: $amount sats -> $addr"
            fi
        done
    fi

    # Check for new payments
    if [ -n "$curr_addrs" ]; then
        for addr in $curr_addrs; do
            if [ -z "$prev_addrs" ] || ! echo "$prev_addrs" | grep -q "^${addr}$"; then
                amount=$(echo "$curr" | jq -r ".[] | select(.address == \"$addr\") | .amount")
                echo "[$(date +%H:%M:%S)] Added: $amount sats -> $addr"
            fi
        done
    fi

    # All done? Only exit if we ever had payments and now have none
    if [ "$curr_count" -eq 0 ] && [ "$ever_had_payments" = true ]; then
        break
    fi

    # Update state
    prev="$curr"
    prev_count="$curr_count"
    prev_addrs="$curr_addrs"

    sleep 30
done

# Final state
echo ""
echo "=== All payments done ==="

mcw_rpc '{"jsonrpc":"2.0","id":"1","method":"stopcoinjoin"}' > /dev/null || exit 1
echo "CoinJoin stopped"
