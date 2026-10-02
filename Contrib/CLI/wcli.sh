#!/usr/bin/env bash
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/rpc-common.sh"
if [[ "${1:-}" == --json ]]; then
    REQUEST=$(cat)
    METHOD=$(printf '%s' "$REQUEST" | jq -er '.method') || exit 1
else
    METHOD="${1:?Usage: wcli.sh command [arguments] or wcli.sh --json < request.json}"
    shift
    PARAMS='[]'
    for value in "$@"; do
        item=$(printf '%s' "$value" | jq -Rsc '. as $text | try fromjson catch $text')
        PARAMS=$(printf '%s' "$PARAMS" | jq -c --argjson item "$item" '. + [$item]')
    done
    REQUEST=$(jq -nc --arg method "$METHOD" --argjson params "$PARAMS" '{jsonrpc:"2.0",id:1,method:$method,params:$params}')
fi
RESULT=$(mcw_rpc_raw "$REQUEST")
CURL_ERRORCODE=$?
RESULT_ERROR=$(printf '%s' "$RESULT" | jq -r .error)

rawprint=(help)
if [ $CURL_ERRORCODE -ne 0 ]; then
    echo "It was not possible to get a response. RPC server could be disabled." >&2
    exit 1
elif [[ "$RESULT_ERROR" == "null" ]]; then
    if [[ " ${rawprint[*]} " =~ ${METHOD} || ${METHOD} == 'query' ]]; then
       echo "$RESULT" | jq -r .result
    else
        IS_NONEMPTY_ARRAY=$(echo "$RESULT" | jq -r '.result | if type=="array" and length > 0 then "true" else "false" end')
        if [[ "$IS_NONEMPTY_ARRAY" == "true" ]]; then
           echo "$RESULT" | jq -r '.result | [.[]| with_entries( .key |= ascii_downcase ) ]
                                         |    (.[0] |keys_unsorted | @tsv)
                                            , (.[]|.|map(.) |@tsv)' | column -t
        else
           echo "$RESULT" | jq -r .result
        fi
    fi
else
   echo "$RESULT_ERROR" | jq -r .message >&2
   exit 1
fi
