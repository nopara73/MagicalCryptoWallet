#!/usr/bin/env python3
"""Exercise the packaged desktop/daemon with a synthetic encrypted regtest wallet.

No installation, startup registration, user wallet, or public Bitcoin network is used.
On Windows also check real window visibility, same-process activation and hide-on-close.
"""
import argparse
import base64
import ctypes
from ctypes import wintypes
from contextlib import contextmanager
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[2]
MNEMONIC = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def require_regtest_p2p_port():
    port = 18444
    with socket.socket() as listener:
        if os.name == "nt":
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        try:
            listener.bind(("127.0.0.1", port))
        except OSError as error:
            raise RuntimeError(f"Regtest P2P port 127.0.0.1:{port} is occupied; leave the existing process running and retry later.") from error
    return port


@contextmanager
def regtest_p2p_port_lease():
    """Serialize synthetic harnesses across checkouts, including node downtime."""
    lease = (Path(tempfile.gettempdir()) / "MagicalCryptoWallet-regtest-p2p-18444.lock").open("a+b")
    if lease.tell() == 0:
        lease.write(b"0")
        lease.flush()
    lease.seek(0)
    try:
        if os.name == "nt":
            import msvcrt
            msvcrt.locking(lease.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError as error:
        lease.close()
        raise RuntimeError("Regtest P2P port 127.0.0.1:18444 is reserved by another synthetic harness; retry later without stopping it.") from error
    try:
        yield
    finally:
        lease.seek(0)
        if os.name == "nt":
            msvcrt.locking(lease.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            fcntl.flock(lease, fcntl.LOCK_UN)
        lease.close()


@contextmanager
def reserve_offline_p2p_port():
    """Keep other nodes out while the client discovers an unavailable peer."""
    with socket.socket() as reservation:
        if os.name == "nt":
            reservation.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
        try:
            reservation.bind(("127.0.0.1", 18444))
        except OSError as error:
            raise RuntimeError("Regtest P2P port 127.0.0.1:18444 was occupied during the offline check; no existing process was stopped.") from error
        yield


def wait_for(check, timeout=45):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            last = check()
            if last:
                return last
        except (OSError, ValueError):
            pass
        time.sleep(.2)
    raise AssertionError(f"Condition timed out after {timeout}s; last result: {last!r}")


def wait_for_rpc_start(process, log_path):
    # HttpListener on Unix accepts connections before its constructor has initialized
    # its connection collection. Poll only after Start has returned, without a sleep.
    def started():
        if process.poll() is not None:
            raise AssertionError(f"Wallet exited with {process.returncode}: {log_path.read_text(encoding='utf-8', errors='replace')[-4000:]}")
        return "JSON-RPC server started." in log_path.read_text(encoding="utf-8", errors="replace")
    wait_for(started)


def rpc(url, method, params=(), allow_error=False, timeout=None):
    request = urllib.request.Request(url, json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        {"Content-Type": "application/json", "Authorization": "Basic " + base64.b64encode(b"synthetic:synthetic").decode()})
    # Mining and password derivation can exceed five seconds on slower CI runners.
    # Give expensive operations one bounded request; never retry a wallet mutation.
    if timeout is None:
        timeout = 30 if method in {"generatetoaddress", "createwallet", "recoverwallet", "build", "send"} else 5
    with urllib.request.urlopen(request, timeout=timeout) as response:
        body = response.read()
    if not body:
        return None
    result = json.loads(body)
    if not allow_error and "error" in result:
        if result["error"].get("code") == -28:
            raise OSError(f"{method}: Bitcoin Core is still warming up: {result['error']['message']}")
        raise AssertionError(f"{method}: {result['error']}")
    return result if allow_error else result.get("result")


def visible_windows(pid):
    if os.name != "nt":
        return []
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    found = []
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(1024)
            user32.GetWindowTextW(hwnd, title, len(title))
            if title.value:
                found.append((hwnd, title.value))
        return True
    user32.EnumWindows(visit, 0)
    return found


def capture_window(hwnd, destination):
    """Render only this application's HWND, even when another window occludes it."""
    from PIL import Image
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
    user32.SetThreadDpiAwarenessContext.argtypes = [ctypes.c_void_p]
    user32.SetThreadDpiAwarenessContext.restype = ctypes.c_void_p
    previous_dpi = user32.SetThreadDpiAwarenessContext(ctypes.c_void_p(-4))
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.GetWindowDC.argtypes = [wintypes.HWND]; user32.GetWindowDC.restype = wintypes.HDC
    user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    user32.PrintWindow.argtypes = [wintypes.HWND, wintypes.HDC, wintypes.UINT]
    gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]; gdi32.CreateCompatibleDC.restype = wintypes.HDC
    gdi32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]; gdi32.CreateCompatibleBitmap.restype = wintypes.HANDLE
    gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HANDLE]; gdi32.SelectObject.restype = wintypes.HANDLE
    gdi32.DeleteObject.argtypes = [wintypes.HANDLE]; gdi32.DeleteDC.argtypes = [wintypes.HDC]
    gdi32.GetDIBits.argtypes = [wintypes.HDC, wintypes.HANDLE, wintypes.UINT, wintypes.UINT, ctypes.c_void_p, ctypes.c_void_p, wintypes.UINT]
    rect = wintypes.RECT()
    assert user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    screen = user32.GetWindowDC(hwnd)
    memory = gdi32.CreateCompatibleDC(screen)
    bitmap = gdi32.CreateCompatibleBitmap(screen, width, height)
    previous = gdi32.SelectObject(memory, bitmap)
    try:
        assert user32.PrintWindow(hwnd, memory, 2), "Could not capture the owned application window."
        gdi32.SelectObject(memory, previous)
        import struct
        header = ctypes.create_string_buffer(struct.pack("<IiiHHIIiiII", 40, width, -height, 1, 32, 0, width * height * 4, 0, 0, 0, 0))
        pixels = ctypes.create_string_buffer(width * height * 4)
        assert gdi32.GetDIBits(screen, bitmap, 0, height, pixels, header, 0) == height
        image = Image.frombytes("RGB", (width, height), pixels.raw, "raw", "BGRX")
        assert image.getcolors(maxcolors=256) is None, "Window capture is blank."
        image.save(destination)
    finally:
        gdi32.SelectObject(memory, previous)
        gdi32.DeleteObject(bitmap); gdi32.DeleteDC(memory); user32.ReleaseDC(hwnd, screen)
        if previous_dpi: user32.SetThreadDpiAwarenessContext(previous_dpi)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--bitcoind", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / ".artifacts/single-wallet/process")
    args = parser.parse_args()
    run = args.output.resolve() / ("synthetic " + uuid.uuid4().hex)
    run.mkdir(parents=True)
    data = run / "MCW data with spaces"; data.mkdir()
    bitcoin = run / "bitcoin"; bitcoin.mkdir()
    suffix = ".exe" if os.name == "nt" else ""
    desktop = args.package.resolve() / ("magicalcryptowallet" + suffix)
    daemon = args.package.resolve() / ("magicalcryptowalletd" + suffix)
    node_p2p = require_regtest_p2p_port()
    node_rpc, wallet_rpc = free_port(), free_port()
    node_url = f"http://127.0.0.1:{node_rpc}/"
    wallet_url = f"http://127.0.0.1:{wallet_rpc}/"
    cli = [f"--datadir={data}", "--network=RegTest"]
    initial = cli + ["--jsonrpcserverenabled=true", "--jsonrpcuser=synthetic", "--jsonrpcpassword=synthetic",
        f"--jsonrpcserverprefixes={wallet_url}",
        "--coordinatoruri=", "--usetor=Disabled", "--exchangerateprovider=None", "--feerateestimationprovider=None", "--downloadnewversion=false", "--enablegpu=false"]
    env = {key: value for key, value in os.environ.items() if not key.startswith("MAGICALCRYPTOWALLET_")}
    env["AVALONIA_TELEMETRY_OPTOUT"] = "1"
    children, logs, results = [], [], {}
    def launch(executable, arguments, name):
        log = (run / (name + ".log")).open("wb"); logs.append(log)
        process = subprocess.Popen([str(executable), *arguments], stdout=log, stderr=subprocess.STDOUT, env=env,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        children.append(process)
        return process
    def stop(process):
        rpc(wallet_url, "stop")
        assert process.wait(timeout=40) == 0
    def ready():
        info = rpc(wallet_url, "getwalletinfo")
        return info if info["state"] == "Ready" else False
    try:
        node = launch(args.bitcoind.resolve(), ["-regtest", f"-datadir={bitcoin}", "-server=1", "-blockfilterindex=1",
            f"-rpcport={node_rpc}", f"-port={node_p2p}", "-rpcuser=synthetic", "-rpcpassword=synthetic", "-fallbackfee=0.0001", "-listen=1", "-bind=127.0.0.1", "-peerblockfilters=1", "-discover=0"], "bitcoin")
        wait_for(lambda: rpc(node_url, "getblockchaininfo"))
        wait_for(lambda: (indexes := rpc(node_url, "getindexinfo")) and all(index["synced"] for index in indexes.values()))
        first = launch(desktop if os.name == "nt" else daemon, initial + (["startsilent"] if os.name == "nt" else []), "first-setup")
        wait_for_rpc_start(first, run / "first-setup.log")
        info = wait_for(lambda: rpc(wallet_url, "getwalletinfo"))
        assert info["state"] == "Unconfigured" and info["balance"] is None
        assert not visible_windows(first.pid), "Hidden first-run setup opened a window."
        rpc(wallet_url, "recoverwallet", [MNEMONIC, "synthetic passphrase"], timeout=60)
        info = wait_for(ready, timeout=360)
        assert info["syncHeight"] == info["targetHeight"] == 0 and info["hasCachedData"]
        assert not info["coinJoinRequiresAuthorization"], "The recovery password did not authorize CoinJoin."
        assert "walletName" not in info and "loaded" not in info
        assert rpc(wallet_url, "loadwallet", allow_error=True)["error"]["code"] == -32601
        results["first_setup_starts_without_window"] = True
        results["setup_password_authorizes_coinjoin_for_current_run"] = True
        # Bitcoin Core's own wallet management is deliberately retained for independent synthetic test participants.
        rpc(node_url, "createwallet", ["synthetic-miner"])
        miner_url = node_url + "wallet/synthetic-miner"
        mining_address = rpc(miner_url, "getnewaddress")
        rpc(node_url, "generatetoaddress", [101, mining_address], timeout=60)
        receive_address = rpc(wallet_url, "getnewaddress", ["synthetic funding", False])["address"]
        rpc(miner_url, "sendtoaddress", [receive_address, .05])
        rpc(node_url, "generatetoaddress", [1, mining_address], timeout=60)
        wait_for(lambda: (info := ready()) and info["balance"] == 5_000_000, timeout=360)
        old_tip = rpc(node_url, "getbestblockhash")
        old_height = rpc(node_url, "getblockcount")
        rpc(node_url, "invalidateblock", [old_tip])
        replacement_address = rpc(miner_url, "getnewaddress")
        rpc(node_url, "generatetoaddress", [2, replacement_address], timeout=60)
        wait_for(lambda: (info := ready()) and info["syncHeight"] == old_height + 1 and info["balance"] == 5_000_000, timeout=360)
        assert rpc(node_url, "getblockhash", [old_height]) != old_tip
        results["p2p_reorg_recovers_funding_on_replacement_chain"] = True
        stop(first)
        first = launch(desktop if os.name == "nt" else daemon, initial + (["startsilent"] if os.name == "nt" else []), "before-signing")
        wait_for_rpc_start(first, run / "before-signing.log")
        info = wait_for(ready, timeout=360)
        assert info["balance"] == 5_000_000 and info["coinJoinRequiresAuthorization"]
        assert not visible_windows(first.pid), "Restarting the encrypted wallet opened a window."
        results["encrypted_startup_synchronizes_without_authorization"] = True
        payment = [{"Sendto": mining_address, "Amount": 500_000, "Label": "synthetic spend"}]
        assert "error" in rpc(wallet_url, "build", [payment, None, 2, "wrong"], allow_error=True, timeout=60)
        assert rpc(wallet_url, "getwalletinfo")["coinJoinRequiresAuthorization"]
        assert rpc(wallet_url, "build", [payment, None, 2, "synthetic passphrase"], timeout=60)
        assert not rpc(wallet_url, "getwalletinfo")["coinJoinRequiresAuthorization"]
        assert "error" in rpc(wallet_url, "build", [payment, None, 2, "wrong"], allow_error=True, timeout=60)
        results["signing_authorizes_coinjoin_but_never_skips_later_password_checks"] = True
        stop(first)
        rpc(node_url, "stop")
        assert node.wait(timeout=20) == 0

        # Persist wallet settings; the Bitcoin transport uses the standard regtest P2P endpoint.
        config_file = data / "Config.RegTest.json"
        config = json.loads(config_file.read_text(encoding="utf-8-sig"))
        config.update(CoordinatorUri="", UseTor="Disabled",
            JsonRpcServerEnabled=True, JsonRpcUser="synthetic", JsonRpcPassword="synthetic", JsonRpcServerPrefixes=[wallet_url],
            DownloadNewVersion=False, EnableGpu=False, ExchangeRateProvider="None", FeeRateEstimationProvider="None")
        config_file.write_text(json.dumps(config), encoding="utf-8")
        (data / "UiConfig.json").write_text(json.dumps(dict(Oobe=False, LastVersionHighlightsDisplayed="99.99.99.0", WindowState="Normal",
            FeeTarget=2, Autocopy=False, AutoPaste=False, IsCustomChangeAddress=False, PrivacyMode=True, DarkModeEnabled=True,
            RunOnSystemStartup=False, HideOnClose=True, SendAmountConversionReversed=False, WindowWidth=1100, WindowHeight=760)), encoding="utf-8")
        with reserve_offline_p2p_port():
            second = launch(desktop if os.name == "nt" else daemon, cli + (["startsilent"] if os.name == "nt" else []), "encrypted-restart")
            wait_for_rpc_start(second, run / "encrypted-restart.log")
            offline = wait_for(lambda: (info := rpc(wallet_url, "getwalletinfo")) and info["state"] == "Offline" and info["hasCachedData"] and info)
            assert offline["balance"] == 5_000_000 and not offline["synchronized"]
            assert offline["coinJoinRequiresAuthorization"], "CoinJoin credentials survived process restart."
            assert rpc(wallet_url, "gethistory")
            assert not visible_windows(second.pid)
            # Finish the initial failed discovery before bringing the peer back.
            wait_for(lambda: "Seeding from DNS" in (run / "encrypted-restart.log").read_text(encoding="utf-8", errors="replace"))
            time.sleep(16)  # The existing discovery attempt is bounded to fifteen seconds.
            assert rpc(wallet_url, "getwalletinfo")["state"] == "Offline"
            results["offline_startup_shows_cached_balance_and_history"] = True
        require_regtest_p2p_port()
        node = launch(args.bitcoind.resolve(), ["-regtest", f"-datadir={bitcoin}", "-server=1", "-blockfilterindex=1",
            f"-rpcport={node_rpc}", f"-port={node_p2p}", "-rpcuser=synthetic", "-rpcpassword=synthetic", "-fallbackfee=0.0001", "-listen=1", "-bind=127.0.0.1", "-peerblockfilters=1", "-discover=0"], "bitcoin-restart")
        wait_for(lambda: rpc(node_url, "getblockchaininfo"))
        rpc(node_url, "loadwallet", ["synthetic-miner"])
        info = wait_for(ready, timeout=360)
        assert info["balance"] == 5_000_000
        assert info["syncHeight"] == info["targetHeight"] == rpc(node_url, "getblockcount")
        assert info["coinJoinRequiresAuthorization"] and not visible_windows(second.pid)
        assert len(list((data / "Wallets/RegTest").glob("*.json"))) == 1
        results["encrypted_restart_synchronizes_before_window"] = True
        if os.name == "nt":
            repeated = launch(desktop, cli + ["startsilent"], "duplicate-silent")
            assert repeated.wait(timeout=8) == 0 and second.poll() is None and not visible_windows(second.pid)
            results["duplicate_silent_is_quiet"] = True
            foreground = launch(desktop, cli, "foreground-activation")
            assert foreground.wait(timeout=8) == 0
            windows = wait_for(lambda: visible_windows(second.pid))
            assert any(title == "Magical Crypto Wallet" for _, title in windows)
            results["foreground_activates_same_process"] = True
            user32 = ctypes.WinDLL("user32", use_last_error=True)
            time.sleep(1)
            capture_window(windows[0][0], run / "masked-dashboard.png")
            user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
            user32.PostMessageW(windows[0][0], 0x0010, 0, 0) # WM_CLOSE, exercising the saved hide-on-close preference.
            wait_for(lambda: not visible_windows(second.pid))
            assert second.poll() is None and ready()
            reopen = launch(desktop, cli, "reopen")
            assert reopen.wait(timeout=8) == 0
            wait_for(lambda: visible_windows(second.pid))
            results["close_and_reopen_keep_session_ready"] = True
            mismatch = launch(desktop, [f"--datadir={data}", "--network=Main"], "wrong-network")
            assert mismatch.wait(timeout=8) != 0
            assert ready()
            results["activation_rejects_other_network"] = True
        conflict = launch(daemon, cli, "daemon-conflict")
        assert conflict.wait(timeout=8) != 0 and second.poll() is None
        results["daemon_cannot_bypass_desktop_lock"] = True
        stop(second)
        final = launch(daemon, cli, "daemon-after-quit")
        wait_for_rpc_start(final, run / "daemon-after-quit.log")
        info = wait_for(ready, timeout=360)
        assert info["coinJoinRequiresAuthorization"]
        if os.name == "nt":
            conflict = launch(desktop, cli, "desktop-daemon-conflict")
            assert conflict.wait(timeout=8) != 0 and final.poll() is None
            results["desktop_cannot_activate_daemon"] = True
        sent = rpc(wallet_url, "send", [payment, None, 2, "synthetic passphrase"], timeout=60)
        wait_for(lambda: sent["txid"] in rpc(node_url, "getrawmempool"))
        rpc(node_url, "generatetoaddress", [1, mining_address])
        wait_for(lambda: (info := ready()) and info["syncHeight"] == rpc(node_url, "getblockcount") and info["balance"] < 5_000_000, timeout=360)
        assert rpc(miner_url, "gettransaction", [sent["txid"]])["confirmations"] >= 1
        results["p2p_send_is_independently_confirmed"] = True
        stop(final)
        results["clean_quit_releases_lock_and_credentials"] = True
        rpc(node_url, "stop")
        assert node.wait(timeout=20) == 0
        (run / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
        print(json.dumps({"results": results, "artifacts": str(run)}))
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
