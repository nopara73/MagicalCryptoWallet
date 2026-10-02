#!/usr/bin/env bash
# Shared root-endpoint requests. Credentials and JSON bodies travel through file descriptors, never curl arguments.
mcw_data_dir="${MAGICALCRYPTOWALLET_DATADIR:-$HOME/.magicalcryptowallet/client}"
mcw_network=$(cat "$mcw_data_dir/network" 2>/dev/null || printf Main)
case "$mcw_network" in
    Main) mcw_config="$mcw_data_dir/Config.json" ;;
    TestNet|RegTest|Signet) mcw_config="$mcw_data_dir/Config.$mcw_network.json" ;;
    *) printf 'Invalid network in data directory.\n' >&2; exit 1 ;;
esac
mcw_config="${MAGICALCRYPTOWALLET_CONFIG:-$mcw_config}"
RPC_ENDPOINT=$(jq -er '.JsonRpcServerPrefixes[0]' "$mcw_config") || exit 1
mcw_basic_auth=$(jq -jr '.JsonRpcUser + ":" + .JsonRpcPassword' "$mcw_config" | base64 | tr -d '\r\n')

mcw_rpc_raw() {
    printf '%s' "$1" | curl --silent --show-error --fail --connect-timeout 3 --max-time 15 \
        --config <(printf 'header = "Authorization: Basic %s"\n' "$mcw_basic_auth") \
        --data-binary @- --header 'Content-Type: application/json' "$RPC_ENDPOINT"
}
mcw_check_result() {
    local error
    printf '%s' "$1" | jq -e 'type == "object" and (has("result") or has("error"))' >/dev/null 2>&1 || {
        printf 'Invalid RPC response.\n' >&2; return 1;
    }
    error=$(printf '%s' "$1" | jq -er '.error.message // empty' 2>/dev/null) || return 0
    printf '%s\n' "$error" >&2
    return 1
}
mcw_rpc() {
    local response
    response=$(mcw_rpc_raw "$1") || return 1
    mcw_check_result "$response" || return 1
    printf '%s\n' "$response"
}
mcw_wait_ready() {
    local deadline=$((SECONDS + 120)) state info
    while (( SECONDS < deadline )); do
        info=$(mcw_rpc '{"jsonrpc":"2.0","id":1,"method":"getwalletinfo"}') || return 1
        state=$(printf '%s' "$info" | jq -r '.result.state')
        if [[ "$state" == Ready ]]; then return 0; fi
        if [[ "$state" == Unconfigured || "$state" == Faulted || "$state" == Stopping ]]; then
            printf 'Wallet is %s: %s\n' "$state" "$(printf '%s' "$info" | jq -r '.result.error // empty')" >&2
            return 1
        fi
        sleep 1
    done
    printf 'Timed out waiting for wallet readiness (%s).\n' "$state" >&2
    return 1
}
mcw_authorized_rpc() {
    local method="$1" parameters="$2" response error passphrase request
    request=$(jq -nc --arg method "$method" --argjson parameters "$parameters" \
        '{jsonrpc:"2.0",id:1,method:$method,params:(if $method=="startcoinjoin" then [null]+$parameters else $parameters+[""] end)}')
    response=$(mcw_rpc_raw "$request") || return 1
    printf '%s' "$response" | jq -e 'type == "object" and (has("result") or has("error"))' >/dev/null 2>&1 || {
        printf 'Invalid RPC response.\n' >&2; return 1;
    }
    error=$(printf '%s' "$response" | jq -r '.error.message // empty')
    if [[ -n "$error" && "$error" =~ [Pp]assword|[Pp]assphrase|[Aa]uthorization ]]; then
        read -r -s -p 'Password: ' passphrase || return 1
        printf '\n' >&2
        request=$(printf '%s' "$passphrase" | jq -Rsc --arg method "$method" --argjson parameters "$parameters" \
            '{jsonrpc:"2.0",id:1,method:$method,params:(if $method=="startcoinjoin" then [.]+$parameters else $parameters+[.] end)}')
        unset passphrase
        response=$(mcw_rpc_raw "$request") || return 1
        unset request
    fi
    mcw_check_result "$response" || return 1
    printf '%s\n' "$response"
}
