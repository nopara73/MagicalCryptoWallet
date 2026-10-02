#!/usr/bin/env python3
"""Observe a packaged Windows desktop using an isolated, preseeded encrypted wallet."""
import faulthandler
import os

_lifecycle_trace = None
if _trace_path := os.environ.get("MCW_LIFECYCLE_TRACE"):
    _lifecycle_trace = open(_trace_path, "w", encoding="utf-8")
    _lifecycle_trace.write("Harness entered before imports.\n")
    _lifecycle_trace.flush()
    faulthandler.dump_traceback_later(30, repeat=True, file=_lifecycle_trace)

import argparse
import base64
import ctypes
from ctypes import wintypes
from contextlib import contextmanager
import json
from pathlib import Path
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


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



def core_rpc(url, method, params=()):
    """Bitcoin Core is only the isolated miner; the desktop exposes no control API."""
    request = urllib.request.Request(url,
        json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        {"Content-Type": "application/json", "Authorization": "Basic " + base64.b64encode(b"synthetic:synthetic").decode()})
    with urllib.request.urlopen(request, timeout=60) as response:
        result = json.loads(response.read())
    if result.get("error"):
        if result["error"]["code"] == -28:
            raise OSError("Bitcoin Core is warming up")
        raise AssertionError(result["error"])
    return result.get("result")


def visible_windows(pid):
    if os.name != "nt":
        return []
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    found = []
    # mcw owns a managed child; inspect the whole owned tree so hidden-startup
    # checks cannot accidentally pass by inspecting only the windowless host.
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    class Entry(ctypes.Structure):
        _fields_ = [("size",wintypes.DWORD),("usage",wintypes.DWORD),("pid",wintypes.DWORD),("heap",ctypes.c_void_p),
                    ("module",wintypes.DWORD),("threads",wintypes.DWORD),("parent",wintypes.DWORD),
                    ("priority",wintypes.LONG),("flags",wintypes.DWORD),("exe",wintypes.WCHAR * 260)]
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.Process32FirstW.argtypes = kernel32.Process32NextW.argtypes = [wintypes.HANDLE,ctypes.POINTER(Entry)]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    snapshot = kernel32.CreateToolhelp32Snapshot(2,0)
    owned = {pid}; parents = {}
    if snapshot == ctypes.c_void_p(-1).value: raise OSError("Process tree snapshot failed")
    try:
        entry = Entry();entry.size = ctypes.sizeof(entry)
        more = kernel32.Process32FirstW(snapshot,ctypes.byref(entry))
        while more:
            parents[entry.pid] = entry.parent
            more = kernel32.Process32NextW(snapshot,ctypes.byref(entry))
        while True:
            descendants = {child for child,parent in parents.items() if parent in owned}
            if descendants <= owned: break
            owned |= descendants
    finally: kernel32.CloseHandle(snapshot)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user32.GetWindowTextW.argtypes = [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int]
    user32.IsWindowVisible.argtypes = [wintypes.HWND]
    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value in owned and user32.IsWindowVisible(hwnd):
            title = ctypes.create_unicode_buffer(1024)
            user32.GetWindowTextW(hwnd, title, len(title))
            if title.value:
                found.append((hwnd, title.value))
        return True
    user32.EnumWindows(visit, 0)
    return found


def window_process_id(hwnd):
    user32 = ctypes.WinDLL("user32", use_last_error=True)
    user32.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    owner = wintypes.DWORD()
    assert user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
    return owner.value


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
    parser.add_argument("--data-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    assert os.name == "nt", "This test exercises the real Windows desktop."
    run, data = args.output.resolve(), args.data_dir.resolve()
    run.mkdir(parents=True, exist_ok=True)
    seed = json.loads((data / "lifecycle-seed.json").read_text(encoding="utf-8-sig"))
    wallet = (data / seed["walletFile"]).resolve()
    assert wallet.is_relative_to(data) and wallet.is_file()
    original = json.loads(wallet.read_text(encoding="utf-8-sig"))
    assert original["EncryptedSecret"]
    user_script = data / "retained-user-script.scm"
    script_contents = user_script.read_bytes()
    bitcoin = run / "bitcoin"; bitcoin.mkdir()
    desktop = args.package.resolve() / "mcw.exe"
    assert desktop.is_file()
    node_p2p = require_regtest_p2p_port()
    node_rpc = free_port()
    node_url = f"http://127.0.0.1:{node_rpc}/"
    cli = [f"--datadir={data}", "--network=RegTest"]
    env = {key: value for key, value in os.environ.items() if not key.startswith("MAGICALCRYPTOWALLET_") and key not in ("MCW_HOSTED", "MCW_HOST_PATH")}
    env["AVALONIA_TELEMETRY_OPTOUT"] = "1"
    children, logs, results = [], [], {}
    def launch(executable, arguments, name):
        log = (run / (name + ".log")).open("wb"); logs.append(log)
        process = subprocess.Popen([str(executable), *arguments], stdout=log, stderr=subprocess.STDOUT, env=env,
            creationflags=subprocess.CREATE_NO_WINDOW)
        children.append(process)
        return process
    def app_log():
        paths = set(data.rglob("Logs*.txt")) | set(run.glob("*.log"))
        return "\n".join(path.read_text(encoding="utf-8-sig", errors="replace") for path in paths)
    def synchronized_height():
        saved = json.loads(wallet.read_text(encoding="utf-8-sig"))
        assert saved["EncryptedSecret"] == original["EncryptedSecret"]
        return int(saved["BlockchainState"]["Height"])
    def expected_saved_height(height):
        return max(0, height - seed["resyncHeightMargin"])
    def confirmed_funding_height(txid):
        for database in data.rglob("Transactions.sqlite"):
            if "ConfirmedTransactions" not in database.parts:
                continue
            try:
                with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
                    row = connection.execute('SELECT block_height FROM "transaction" WHERE txid=? AND block_hash IS NOT NULL',
                        (bytes.fromhex(txid),)).fetchone()
                if row:
                    return row[0]
            except sqlite3.OperationalError:
                pass
        return None
    def alive(process):
        assert process.poll() is None, app_log()[-4000:]
        return True
    def foreground(owner, name):
        activation = launch(desktop, cli, name)
        assert activation.wait(timeout=15) == 0 and alive(owner)
        windows = wait_for(lambda: visible_windows(owner.pid))
        assert len(windows) == 1 and windows[0][1] == "Magical Crypto Wallet", windows
        return windows[0][0]
    def close_window(hwnd):
        user32 = ctypes.WinDLL("user32", use_last_error=True)
        user32.PostMessageW.argtypes = [wintypes.HWND, wintypes.UINT, wintypes.WPARAM, wintypes.LPARAM]
        assert user32.PostMessageW(hwnd, 0x0010, 0, 0)
    try:
        help_result = subprocess.run([str(desktop), "--help"], capture_output=True, text=True, env=env, check=True, timeout=30)
        forbidden = ("jsonrpc", "rpconion", "experimentalfeatures", "scripting", "daemon")
        assert not any(option in help_result.stdout.lower() for option in forbidden)
        desktop_help = subprocess.run([str(desktop), "gui", *cli, "--help"], capture_output=True, text=True, env=env, check=True, timeout=30)
        help_text = (desktop_help.stdout + desktop_help.stderr).lower()
        assert all(option in help_text for option in ("--loglevel", "--network"))
        assert not any(option in help_text for option in forbidden)
        version = subprocess.run([str(desktop), "gui", *cli, "--version"], capture_output=True, text=True, env=env, check=True, timeout=30)
        host_version = subprocess.run([str(desktop), "--version"], capture_output=True, text=True, env=env, check=True, timeout=30).stdout.strip().split()[-1]
        assert host_version in version.stdout + version.stderr
        results["help_omits_retired_options"] = True
        node = launch(args.bitcoind.resolve(), ["-regtest", f"-datadir={bitcoin}", "-server=1", "-blockfilterindex=1",
            f"-rpcport={node_rpc}", f"-port={node_p2p}", "-rpcuser=synthetic", "-rpcpassword=synthetic",
            "-fallbackfee=0.0001", "-listen=1", "-bind=127.0.0.1", "-peerblockfilters=1", "-discover=0"], "bitcoin")
        wait_for(lambda: core_rpc(node_url, "getblockchaininfo"))
        core_rpc(node_url, "createwallet", ["synthetic-miner"])
        miner = node_url + "wallet/synthetic-miner"
        address = core_rpc(miner, "getnewaddress")
        core_rpc(node_url, "generatetoaddress", [101, address])
        funding = core_rpc(miner, "sendtoaddress", [seed["receiveAddress"], .05])
        core_rpc(node_url, "generatetoaddress", [1, address])
        height = core_rpc(node_url, "getblockcount")
        first = launch(desktop, cli + ["startsilent"], "hidden-start")
        wait_for(lambda: alive(first) and synchronized_height() == expected_saved_height(height) and "is fully synchronized." in app_log(), timeout=360)
        assert not visible_windows(first.pid), "Hidden encrypted startup opened a window or authorization prompt."
        wait_for(lambda: confirmed_funding_height(funding) == height)
        results["encrypted_hidden_startup_synchronizes_through_p2p"] = True
        repeated = launch(desktop, cli + ["startsilent"], "duplicate-silent")
        assert repeated.wait(timeout=15) == 0 and alive(first) and not visible_windows(first.pid)
        results["duplicate_silent_is_quiet"] = True
        hwnd = foreground(first, "foreground-activation")
        time.sleep(1)
        capture_window(hwnd, run / "masked-dashboard.png")
        results["foreground_activates_same_process"] = True
        close_window(hwnd)
        wait_for(lambda: not visible_windows(first.pid))
        assert alive(first)
        core_rpc(node_url, "generatetoaddress", [1, address])
        height += 1
        wait_for(lambda: synchronized_height() == expected_saved_height(height), timeout=360)
        hwnd = foreground(first, "reopen")
        results["hide_and_reopen_keep_synchronization_running"] = True
        mismatch = launch(desktop, [f"--datadir={data}", "--network=Main"], "wrong-network")
        assert mismatch.wait(timeout=15) != 0 and alive(first)
        results["activation_rejects_other_network"] = True
        settings = subprocess.run(["powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
            str(ROOT / "Contrib/Tests/close-background-window.ps1"), "-WindowHandle", str(hwnd), "-OwnerProcessId", str(window_process_id(hwnd))],
            timeout=45, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        settings_log = settings.stdout + "\n" + settings.stderr
        (run / "settings-shutdown.log").write_text(settings_log, encoding="utf-8")
        assert settings.returncode == 0, settings_log
        wait_for(lambda: not json.loads((data / "UiConfig.json").read_text(encoding="utf-8-sig"))["HideOnClose"])
        close_window(hwnd)
        assert first.wait(timeout=60) == 0
        assert "WalletSession is stopped." in app_log()
        results["normal_window_shutdown_stops_wallet_services"] = True
        final = launch(desktop, cli + ["startsilent"], "after-quit")
        wait_for(lambda: alive(final) and synchronized_height() == expected_saved_height(height) and "is fully synchronized." in (run / "after-quit.log").read_text(encoding="utf-8-sig", errors="replace"), timeout=120)
        assert not visible_windows(final.pid)
        hwnd = foreground(final, "activation-after-quit")
        close_window(hwnd)
        assert final.wait(timeout=60) == 0
        results["normal_shutdown_releases_lock_for_restart"] = True
        saved = json.loads(wallet.read_text(encoding="utf-8-sig"))
        assert saved["EncryptedSecret"] == original["EncryptedSecret"]
        assert user_script.read_bytes() == script_contents
        results["encrypted_wallet_and_user_script_preserved"] = True
        core_rpc(node_url, "stop")
        assert node.wait(timeout=30) == 0
        (run / "results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
        print(json.dumps({"results": results, "artifacts": str(run)}))
    finally:
        # Clean up only processes created by this harness, including when an assertion fails.
        for process in reversed(children):
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=10)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=10)
        for log in logs: log.close()


if __name__ == "__main__":
    try:
        with regtest_p2p_port_lease():
            main()
    finally:
        if _lifecycle_trace is not None:
            faulthandler.cancel_dump_traceback_later()
            _lifecycle_trace.close()
