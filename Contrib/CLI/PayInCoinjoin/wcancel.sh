#!/usr/bin/env bash

# MagicalCryptoWallet Cancel Payments in CoinJoin
# Interactive selection to cancel pending payments

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
echo ""
echo "=== Pending Payments ==="
echo ""

# Get pending payments
result=$(mcw_rpc '{"jsonrpc":"2.0","id":"1","method":"listpaymentsincoinjoin"}')
error=$(echo "$result" | jq -r '.error.message // empty')

if [ -n "$error" ]; then
    echo "Error: $error"
    exit 1
fi

payments=$(echo "$result" | jq -r '.result')
count=$(echo "$payments" | jq 'length')

if [ "$count" -eq 0 ]; then
    echo "No pending payments."
    exit 0
fi

# Display numbered list
for i in $(seq 0 $((count - 1))); do
    num=$((i + 1))
    amount=$(echo "$payments" | jq -r ".[$i].amount")
    address=$(echo "$payments" | jq -r ".[$i].address")
    echo "  [$num] $amount sats -> $address"
done

echo ""
echo "  [A] Cancel all"
echo "  [Q] Quit"
echo ""
read -p "Cancel which? " choice

# Quit
if [[ "${choice^^}" == "Q" ]]; then
    exit 0
fi

# Cancel all
if [[ "${choice^^}" == "A" ]]; then
    echo ""
    ids=$(echo "$payments" | jq -r '.[].id')
    for id in $ids; do
        mcw_authorized_rpc cancelpaymentincoinjoin "$(jq -nc --arg id "$id" '[$id]')" > /dev/null || exit 1
        amount=$(echo "$payments" | jq -r ".[] | select(.id == \"$id\") | .amount")
        address=$(echo "$payments" | jq -r ".[] | select(.id == \"$id\") | .address")
        echo "Cancelled: $amount sats -> $address"
    done
    exit 0
fi

# Cancel specific numbers (comma or space separated)
selections=$(echo "$choice" | tr ',' ' ')

for sel in $selections; do
    if ! [[ "$sel" =~ ^[0-9]+$ ]]; then
        echo "Invalid selection: $sel"
        continue
    fi

    idx=$((sel - 1))

    if [ "$idx" -lt 0 ] || [ "$idx" -ge "$count" ]; then
        echo "Invalid selection: $sel"
        continue
    fi

    id=$(echo "$payments" | jq -r ".[$idx].id")
    amount=$(echo "$payments" | jq -r ".[$idx].amount")
    address=$(echo "$payments" | jq -r ".[$idx].address")

    cancel_result=$(mcw_authorized_rpc cancelpaymentincoinjoin "$(jq -nc --arg id "$id" '[$id]')") || continue
    cancel_error=$(echo "$cancel_result" | jq -r '.error.message // empty')

    if [ -n "$cancel_error" ]; then
        echo "Failed to cancel [$sel]: $cancel_error"
    else
        echo "Cancelled: $amount sats -> $address"
    fi
done
