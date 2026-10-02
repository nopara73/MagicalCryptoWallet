#!/usr/bin/env python3
"""Verify Send authorizes automatic CoinJoin in five independent encrypted regtest clients."""
import argparse
import json
import os
from pathlib import Path
import runpy
import subprocess
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
helpers = runpy.run_path(str(Path(__file__).with_name("test-single-wallet-process.py")))
free_port, wait_for, rpc, require_regtest_p2p_port = (helpers[name] for name in ("free_port", "wait_for", "rpc", "require_regtest_p2p_port"))
regtest_p2p_port_lease = helpers["regtest_p2p_port_lease"]
PASSWORD = "synthetic CoinJoin passphrase"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--bitcoind", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / ".artifacts/single-wallet/coinjoin")
    parser.add_argument("--timeout", type=int, default=600)
    args = parser.parse_args()
    run = args.output.resolve() / ("synthetic " + uuid.uuid4().hex)
    run.mkdir(parents=True)
    bitcoin = run / "bitcoin"; bitcoin.mkdir()
    coordinator_data = run / "coordinator"; coordinator_data.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    package = args.package.resolve()
    daemon, coordinator = (package / (name + suffix) for name in ("magicalcryptowalletd", "magicalcryptowallet-coordinator"))
    node_p2p = require_regtest_p2p_port()
    node_rpc, coordinator_port = free_port(), free_port()
    node_url = f"http://127.0.0.1:{node_rpc}/"
    coordinator_url = f"http://127.0.0.1:{coordinator_port}/"
    config = dict(Network="RegTest", MainNetBitcoinRpcUri=node_url, TestNetBitcoinRpcUri=node_url,
        RegTestBitcoinRpcUri=node_url, BitcoinRpcConnectionString="synthetic:synthetic", ConfirmationTarget=108,
        DoSSeverity="0.10", DoSMinTimeForFailedToVerify="31d 0h 0m 0s", DoSMinTimeForCheating="1d 0h 0m 0s",
        DoSPenaltyFactorForDisruptingConfirmation=.2, DoSPenaltyFactorForDisruptingSignalReadyToSign=1,
        DoSPenaltyFactorForDisruptingSigning=1, DoSPenaltyFactorForDisruptingByDoubleSpending=3,
        DoSMinTimeInPrison="0d 0h 20m 0s", MinRegistrableAmount="0.00005", MaxRegistrableAmount="43000",
        AllowNotedInputRegistration=True, MaxInputCountByRound=10, MinInputCountByRoundMultiplier=.5,
        StandardInputRegistrationTimeout="0d 0h 2m 0s",
        BlameInputRegistrationTimeout="0d 0h 1m 0s", ConnectionConfirmationTimeout="0d 0h 1m 0s",
        OutputRegistrationTimeout="0d 0h 1m 0s", TransactionSigningTimeout="0d 0h 1m 0s",
        FailFastOutputRegistrationTimeout="0d 0h 1m 0s", FailFastTransactionSigningTimeout="0d 0h 1m 0s",
        RoundExpiryTimeout="0d 0h 5m 0s", MinInputCountByBlameRoundMultiplier=.4, RoundDestroyerThreshold=375,
        MaxSuggestedAmountBase="100", CollectCoordinatorFees=False, CoordinatorExtPubKey=None,
        CoordinatorExtPubKeyCurrentDepth=1, RoundParallelization=1, CoordinatorIdentifier="CoinJoinCoordinatorIdentifier",
        AllowP2wpkhInputs=True, AllowP2trInputs=True, AllowP2wpkhOutputs=True, AllowP2trOutputs=True,
        AllowP2pkhOutputs=False, AllowP2shOutputs=False, AllowP2wshOutputs=False, DelayTransactionSigning=False,
        AnnouncerConfig=dict(CoordinatorName="Synthetic", IsEnabled=False, CoordinatorDescription="Local test",
            CoordinatorUri=coordinator_url, AbsoluteMinInputCount=5, ReadMoreUri=coordinator_url, RelayUris=[], Key=""),
        PublishAsOnionService=False)
    (coordinator_data / "Config.json").write_text(json.dumps(config), encoding="utf-8")
    env = {key: value for key, value in os.environ.items() if not key.startswith("MAGICALCRYPTOWALLET_")}
    children, logs, clients, sends = [], [], [], []

    def launch(executable, arguments, name):
        log = (run / (name + ".log")).open("wb"); logs.append(log)
        process = subprocess.Popen([str(executable), *arguments], stdout=log, stderr=subprocess.STDOUT, env=env,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        children.append(process)
        return process

    def ready(url, minimum_balance=0):
        info = rpc(url, "getwalletinfo")
        return info if info["state"] == "Ready" and info["balance"] >= minimum_balance else False

    try:
        node = launch(args.bitcoind.resolve(), ["-regtest", f"-datadir={bitcoin}", "-server=1", "-blockfilterindex=1",
            f"-rpcport={node_rpc}", f"-port={node_p2p}", "-rpcuser=synthetic", "-rpcpassword=synthetic",
            "-fallbackfee=0.0001", "-listen=1", "-bind=127.0.0.1", "-peerblockfilters=1", "-discover=0"], "bitcoin")
        wait_for(lambda: rpc(node_url, "getblockchaininfo"))
        # This is Bitcoin Core's independent miner, not an application wallet-selection command.
        rpc(node_url, "createwallet", ["synthetic-miner"])
        miner_url = node_url + "wallet/synthetic-miner"
        mining_address = rpc(miner_url, "getnewaddress")
        rpc(node_url, "generatetoaddress", [150, mining_address])
        wait_for(lambda: (indexes := rpc(node_url, "getindexinfo")) and all(index["synced"] for index in indexes.values()))
        for index in range(5):
            data = run / ("client " + str(index)); data.mkdir()
            url = f"http://127.0.0.1:{free_port()}/"
            cli = [f"--datadir={data}", "--network=RegTest", "--jsonrpcserverenabled=true", "--jsonrpcuser=synthetic",
                "--jsonrpcpassword=synthetic", f"--jsonrpcserverprefixes={url}", f"--coordinatoruri={coordinator_url}",
                "--usetor=Disabled", "--exchangerateprovider=None", "--feerateestimationprovider=None", "--downloadnewversion=false",
                "--maxcoinjoinminingfeerate=500", "--absolutemininputcount=4", "--enablegpu=false"]
            process = launch(daemon, cli, f"setup-{index}")
            wait_for(lambda: rpc(url, "getwalletinfo"))
            rpc(url, "createwallet", [PASSWORD])
            wait_for(lambda: ready(url), timeout=360)
            rpc(url, "stop")
            assert process.wait(timeout=40) == 0
            wallet_file = data / "Wallets/RegTest/Wallet.json"
            wallet = json.loads(wallet_file.read_text(encoding="utf-8-sig"))
            wallet.update(AutoCoinJoin=True, AnonScoreTarget=5)
            wallet_file.write_text(json.dumps(wallet), encoding="utf-8")
            process = launch(daemon, cli, f"client-{index}")
            info = wait_for(lambda: ready(url), timeout=360)
            assert info["coinJoinRequiresAuthorization"] and info["coinjoinStatus"] == "Idle"
            for _ in range(4):
                address = rpc(url, "getnewaddress", ["synthetic funding"])["address"]
                rpc(miner_url, "sendtoaddress", [address, 1.0])
            clients.append((process, url))
            print(f"Client {index}: encrypted startup synchronized without authorization.", flush=True)
        rpc(node_url, "generatetoaddress", [1, mining_address])
        for process, url in clients:
            wait_for(lambda: ready(url, 400_000_000), timeout=360)
            assert process.poll() is None and rpc(url, "getwalletinfo")["coinJoinRequiresAuthorization"]
            destination = rpc(miner_url, "getnewaddress")
            payment = [dict(Sendto=destination, Amount=500_000, Label="synthetic send", SubtractFee=False)]
            sends.append(rpc(url, "send", [payment, None, 2, PASSWORD])["txid"])
            assert not rpc(url, "getwalletinfo")["coinJoinRequiresAuthorization"]
            wrong = rpc(url, "build", [payment, None, 2, "incorrect passphrase"], allow_error=True)
            assert "error" in wrong, "A previous Send skipped a later passphrase check."
            print(f"Client {len(sends) - 1}: Send authorized automatic CoinJoin; later wrong password rejected.", flush=True)
        for _, url in clients:
            wait_for(lambda: rpc(url, "getwalletinfo")["coinjoinStatus"] != "Idle", timeout=360)
        rpc(node_url, "generatetoaddress", [1, mining_address])
        # All clients are authorized before opening a round, so setup speed cannot split participants across rounds.
        service = launch(coordinator, [f"--datadir={coordinator_data}", f"--urls={coordinator_url}"], "coordinator")
        def coordinator_ready():
            assert service.poll() is None, "The isolated coordinator exited before becoming ready."
            request = urllib.request.Request(coordinator_url + "wabisabi/status", b'{"RoundCheckpoints":[]}',
                {"Content-Type": "application/json"})
            with urllib.request.urlopen(request, timeout=5) as response:
                return response.status == 200
        wait_for(coordinator_ready)
        started = time.monotonic()
        coinjoins = []
        while time.monotonic() - started < args.timeout:
            assert service.poll() is None and all(process.poll() is None for process, _ in clients)
            for txid in rpc(node_url, "getrawmempool"):
                if txid in sends or txid in coinjoins: continue
                transaction = rpc(node_url, "getrawtransaction", [txid, True])
                if len(transaction["vin"]) >= 5 and len(transaction["vout"]) >= 5:
                    coinjoins.append(txid)
                    # Confirm the coordinator broadcast and verify it through client P2P filters.
                    rpc(node_url, "generatetoaddress", [1, mining_address])
            if coinjoins and all(any(coin["anonymityScore"] > 1 for coin in rpc(url, "listcoins")) for _, url in clients):
                break
            time.sleep(.5)
        assert coinjoins, "No CoinJoin broadcast before the deadline."
        rpc(node_url, "generatetoaddress", [1, mining_address])
        for _, url in clients:
            wait_for(lambda: ready(url), timeout=360)
            wait_for(lambda: any(coin["anonymityScore"] > 1 and coin["confirmed"] for coin in rpc(url, "listcoins")))
            rpc(url, "stopcoinjoin")
        for process, url in clients:
            rpc(url, "stop")
            assert process.wait(timeout=45) == 0
        rpc(node_url, "stop"); assert node.wait(timeout=20) == 0
        results = dict(independent_clients=5, encrypted_startup_without_authorization=True,
            send_authorizes_automatic_coinjoin=True, later_wrong_password_rejected=True,
            every_client_received_confirmed_mixed_outputs=True, coinjoin_transactions=coinjoins)
        (run / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
        print(json.dumps(dict(results=results, artifacts=str(run))))
    finally:
        for process in reversed(children):
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=10)
        for log in logs: log.close()


if __name__ == "__main__":
    with regtest_p2p_port_lease():
        main()
