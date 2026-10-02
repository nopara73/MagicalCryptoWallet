#!/bin/bash

set -e

# Configuration
BITCOIN_DATADIR="/tmp/bitcoin-regtest"
MAGICALCRYPTOWALLET_DATADIR="/tmp/magicalcryptowallet"
BITCOIN_RPC_PORT=18443
BITCOIN_P2P_PORT=18444
COORDINATOR_PORT=38126
MAGICALCRYPTOWALLET_WALLET_RPC_PORT=38128
NUM_CLIENTS=5
WALLET_PIDS=()

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

cleanup() {
    kill $COORDINATOR_PID
    for pid in "${WALLET_PIDS[@]}"; do kill "$pid"; done
    bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest stop

    rm -rf $BITCOIN_DATADIR
    exit
}
trap cleanup EXIT

echo -e "${YELLOW}Starting Bitcoin node in regtest...${NC}"
mkdir -p "$BITCOIN_DATADIR"

bitcoind \
  -regtest \
  -datadir="$BITCOIN_DATADIR" \
  -rpcport=$BITCOIN_RPC_PORT \
  -port=$BITCOIN_P2P_PORT \
  -server \
  -rpcuser=regtest \
  -rpcpassword=regtest \
  -blockfilterindex=1 \
  -fallbackfee=0.0001 \
  -daemon

sleep 5

echo -e "${GREEN}✓ Bitcoin node started${NC}"

# Wait for bitcoin to be ready
echo -e "${YELLOW}Waiting for Bitcoin RPC to be ready...${NC}"
for i in {1..30}; do
  if bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest getblockchaininfo &>/dev/null; then
    echo -e "${GREEN}✓ Bitcoin RPC is ready${NC}"
    break
  fi
  sleep 1
done

# Create default wallet
echo -e "${YELLOW}Creating default wallet...${NC}"
bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest createwallet "default" > /dev/null 2>&1 || true

# Create default wallet
echo -e "${YELLOW}Loading default wallet...${NC}"
bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest loadwallet "default" > /dev/null 2>&1 || true

# Generate some blocks to have coins
echo -e "${YELLOW}Generating initial blocks...${NC}"
bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest generatetoaddress 150 $(bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest -rpcwallet="default" getnewaddress) > /dev/null

echo -e "${YELLOW}Build all MagicalCryptoWallet projects from source...${NC}"
dotnet build > /dev/null 2>&1 || { echo -e "${RED}Failed to build.${NC}"; exit 1; }

echo -e "${YELLOW}Starting MagicalCryptoWallet Coordinator...${NC}"
mkdir -p "$MAGICALCRYPTOWALLET_DATADIR/Coordinator"

# Start coordinator in background
MAGICALCRYPTOWALLET_COORDINATOR_DATADIR="$MAGICALCRYPTOWALLET_DATADIR/Coordinator"
MAGICALCRYPTOWALLET_COORDINATOR_LOGFILE="$MAGICALCRYPTOWALLET_COORDINATOR_DATADIR/Logs.txt"
rm -f "$MAGICALCRYPTOWALLET_COORDINATOR_LOGFILE"

cat > $MAGICALCRYPTOWALLET_COORDINATOR_DATADIR/Config.json << EOF
{
  "Network": "RegTest",
  "MainNetBitcoinRpcUri": "http://localhost:$BITCOIN_RPC_PORT",
  "TestNetBitcoinRpcUri": "http://localhost:$BITCOIN_RPC_PORT",
  "RegTestBitcoinRpcUri": "http://localhost:$BITCOIN_RPC_PORT",
  "BitcoinRpcConnectionString": "regtest:regtest",
  "ConfirmationTarget": 108,
  "DoSSeverity": "0.10",
  "DoSMinTimeForFailedToVerify": "31d 0h 0m 0s",
  "DoSMinTimeForCheating": "1d 0h 0m 0s",
  "DoSPenaltyFactorForDisruptingConfirmation": 0.2,
  "DoSPenaltyFactorForDisruptingSignalReadyToSign": 1,
  "DoSPenaltyFactorForDisruptingSigning": 1,
  "DoSPenaltyFactorForDisruptingByDoubleSpending": 3,
  "DoSMinTimeInPrison": "0d 0h 20m 0s",
  "MinRegistrableAmount": "0.00005",
  "MaxRegistrableAmount": "43000.00",
  "AllowNotedInputRegistration": true,
  "StandardInputRegistrationTimeout": "0d 0h 3m 0s",
  "BlameInputRegistrationTimeout": "0d 0h 3m 0s",
  "ConnectionConfirmationTimeout": "0d 0h 1m 0s",
  "OutputRegistrationTimeout": "0d 0h 1m 0s",
  "TransactionSigningTimeout": "0d 0h 1m 0s",
  "FailFastOutputRegistrationTimeout": "0d 0h 3m 0s",
  "FailFastTransactionSigningTimeout": "0d 0h 1m 0s",
  "RoundExpiryTimeout": "0d 0h 5m 0s",
  "MaxInputCountByRound": 10,
  "MinInputCountByRoundMultiplier": 0.5,
  "MinInputCountByBlameRoundMultiplier": 0.4,
  "RoundDestroyerThreshold": 375,
  "CollectCoordinatorFees": false,
  "CoordinatorExtPubKey": null,
  "CoordinatorExtPubKeyCurrentDepth": 1,
  "MaxSuggestedAmountBase": "100",
  "RoundParallelization": 1,
  "CoordinatorIdentifier": "CoinJoinCoordinatorIdentifier",
  "AllowP2wpkhInputs": true,
  "AllowP2trInputs": true,
  "AllowP2wpkhOutputs": true,
  "AllowP2trOutputs": true,
  "AllowP2pkhOutputs": false,
  "AllowP2shOutputs": false,
  "AllowP2wshOutputs": false,
  "DelayTransactionSigning": false,
  "AnnouncerConfig": {
    "CoordinatorName": "Coordinator",
    "IsEnabled": false,
    "CoordinatorDescription": "WabiSabi Coinjoin Coordinator",
    "CoordinatorUri": "https://api.example.com/",
    "AbsoluteMinInputCount": 21,
    "ReadMoreUri": "https://api.example.com/",
    "RelayUris": [
      "wss://relay.primal.net"
    ],
    "Key": "nsec1wax9zrs4r8g57767760j3drg87hgm5mwqtecznxtarrt9zsl6fhqyfdh7l"
  },
  "PublishAsOnionService": false,
  "OnionServicePrivateKey": null
}
EOF

export ASPNETCORE_HTTP_PORTS="$COORDINATOR_PORT"
dotnet run --project MagicalCryptoWallet.Coordinator -- --logevel=debug --datadir="$MAGICALCRYPTOWALLET_COORDINATOR_DATADIR" &> "$MAGICALCRYPTOWALLET_COORDINATOR_DATADIR/stdout.log" &
COORDINATOR_PID=$!

sleep 5
echo -e "${GREEN}✓ Coordinator started (PID: $COORDINATOR_PID; Directory: $MAGICALCRYPTOWALLET_COORDINATOR_DATADIR)${NC}"

echo -e "${YELLOW}Starting independent single-wallet clients${NC}"
for (( client = 0; client < NUM_CLIENTS; client++ )); do
  client_dir="$MAGICALCRYPTOWALLET_DATADIR/Client$client"
  port=$((MAGICALCRYPTOWALLET_WALLET_RPC_PORT + client))
  mkdir -p "$client_dir"
  dotnet run --project MagicalCryptoWallet.Daemon --no-build -- \
    --loglevel=trace \
    --network=regtest \
    --coordinatorUri="http://127.0.0.1:$COORDINATOR_PORT" \
    --bitcoinrpcendpoint="http://127.0.0.1:$BITCOIN_RPC_PORT/" \
    --bitcoinrpccredentialstring="regtest:regtest" \
    --jsonrpcserverprefixes="http://127.0.0.1:$port/" \
    --datadir="$client_dir" \
    --jsonrpcserverenabled=true \
    --maxcoinjoinminingfeerate=500 \
    --absolutemininputcount=4 \
    --usetor="disabled" > "$client_dir/stdout.log" 2>&1 &
  WALLET_PIDS+=("$!")
done
sleep 5

echo -e "${YELLOW}Creating Magical Crypto Wallets${NC}"

# Function to start a wallet and perform coinjoin
create_and_fund_wallet() {
  local client=$1
  local wallet_name="wallet$client"
  local port=$((MAGICALCRYPTOWALLET_WALLET_RPC_PORT + client))

  echo -e "${YELLOW}Creating MagicalCryptoWallet wallet $wallet_name...${NC}"
  local request="{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"createwallet\",\"params\":[\"\"]}"
  echo "→ $request"

  local response=$(curl -s -X POST "http://127.0.0.1:$port/" -H "Content-Type: application/json" -d "$request")
  echo "← $response"

  echo -e "${YELLOW}Generating a block to make sure wallet loading will succeed...${NC}"
  bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest generatetoaddress 1 $(bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest -rpcwallet="default" getnewaddress) > /dev/null

  local deadline=$((SECONDS + 120))
  while (( SECONDS < deadline )); do
    local info=$(curl -s --max-time 5 -X POST "http://127.0.0.1:$port/" -H "Content-Type: application/json" -d '{"jsonrpc":"2.0","id":2,"method":"getwalletinfo"}')
    if [[ $(echo "$info" | jq -r '.result.state') == "Ready" ]]; then break; fi
    sleep 1
  done
  if [[ $(echo "$info" | jq -r '.result.state') != "Ready" ]]; then
    echo "Client $client did not become ready: $info" >&2
    exit 1
  fi

  local i
  for (( i = 0; i < 4; i++ )); do
    echo -e "${YELLOW}Generating address #$i for $wallet_name...${NC}"
    local request='{"jsonrpc":"2.0","id":"3","method":"getnewaddress","params":["label"]}'
    echo "→ $request"
    local response=$(curl -s -X POST http://127.0.0.1:$port/ -H "Content-Type: application/json" -d "$request")
    echo "← $response"

    local address=$(echo "$response" | jq -r '.result.address')

    if [[ -z "$address" || "$address" == "null" ]]; then
        echo -e "${RED}Error: Failed to get new address for wallet '$wallet_name'${NC}"
        exit 1
    fi

    echo -e "${YELLOW}Sending funds to $wallet_name ($address)...${NC}"
    bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest -rpcwallet="default" \
      sendtoaddress "$address" 1.0 > /dev/null
  done
}

start_coinjoin()
{
  local client=$1
  local wallet_name="wallet$client"
  local port=$((MAGICALCRYPTOWALLET_WALLET_RPC_PORT + client))
  echo -e "${YELLOW}Starting coinjoin...${NC}"
  curl -s -X POST http://127.0.0.1:$port/ \
      -H "Content-Type: application/json" \
      -d '{"jsonrpc":"2.0","id":"1","method":"startcoinjoin","params":[]}' > /dev/null

  echo -e "${GREEN}✓ Coinjoin initiated for $wallet_name${NC}"
}

# Set up one wallet per client and initiate coinjoins
echo -e "${YELLOW}Setting up one wallet in each independent client...${NC}"

for (( i = 0; i < NUM_CLIENTS; i++ )); do
  create_and_fund_wallet "$i"
  sleep 2
done

echo -e "${GREEN}✓ Wallets created and well funded${NC}"

# Generate a block to confirm
echo -e "${YELLOW}Mine a new block to confirm all transactions${NC}"
bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest \
  generatetoaddress 1 $(bitcoin-cli -regtest -rpcport=$BITCOIN_RPC_PORT -rpcuser=regtest -rpcpassword=regtest -rpcwallet="default" getnewaddress) > /dev/null

sleep 2

echo -e "${YELLOW}Starting coinjoins...${NC}"
for (( i = 0; i < NUM_CLIENTS; i++ )); do
  start_coinjoin "$i" &
  sleep 2
done

echo -e "${GREEN}✓ All single-wallet clients started and coinjoins initiated${NC}"
echo -e "${YELLOW}Bitcoin node PID: $BITCOIN_PID${NC}"
echo -e "${YELLOW}Coordinator PID: $COORDINATOR_PID${NC}"
echo -e "${YELLOW}Magical Crypto Wallet Daemon PID: ${WALLET_PIDS[*]}${NC}"
echo -e "${YELLOW}Bitcoin datadir: $BITCOIN_DATADIR${NC}"
echo -e "${YELLOW}MagicalCryptoWallet datadir: $MAGICALCRYPTOWALLET_DATADIR${NC}"


# Keep script running
echo -e "${GREEN}✓ Setup complete.${NC}"
echo -e "${YELLOW}Wait for coinjoin, or press Ctrl+C to stop all services.${NC}"
TEST_TIMEOUT=600

timeout $TEST_TIMEOUT tail -f "$MAGICALCRYPTOWALLET_COORDINATOR_LOGFILE" | grep -q "Successfully broadcast the coinjoin"

if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ WE HAVE A COINJOIN!!!!${NC}"
else
  echo -e "${RED}Timeout: No coinjoin after $TEST_TIMEOUT seconds.${NC}"
  exit 1
fi
