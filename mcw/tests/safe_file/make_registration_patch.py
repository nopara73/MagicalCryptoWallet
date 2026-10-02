"""Propose shared changes without editing shared source files. Development only."""
import argparse
import difflib
import pathlib
import shutil
import subprocess


def once(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError("shared host changed; review registration instead of guessing")
    return text.replace(old, new, 1)


def app_registration(text):
    if "safe_files: &mut SafeFiles" in text:
        return text
    text = once(text, "type Handoff = Option<(u16, Vec<String>)>;\n", "type Handoff = Option<(u16, Vec<String>)>;\ntype SafeFiles = crate::safe_file_service::payload::Dispatch<platform::safe_file::NativeFileSystem>;\n")
    text = once(text, "    let mut stopping = None;\n", "    let mut stopping = None;\n    let mut safe_files = SafeFiles::new(platform::safe_file::NativeFileSystem);\n")
    text = once(text, "        if let Some(status) = child.process.try_wait()? {\n", "        if let Some(status) = child.process.try_wait()? {\n            safe_files.close();\n")
    # Both actual dispatch sites must receive the same per-connection owner.
    if text.count("                    bootstrap,\n") != 2:
        raise RuntimeError("shared dispatch sites changed")
    text = text.replace("                    bootstrap,\n", "                    bootstrap,\n                    &mut safe_files,\n")
    text = once(text, "                    eprintln!(\"mcw: {error}\");\n", "                    safe_files.close();\n                    eprintln!(\"mcw: {error}\");\n")
    if "            Event::Closed(failure) => {\n" in text:
        text = once(text, "            Event::Closed(failure) => {\n", "            Event::Closed(failure) => {\n                safe_files.close();\n")
    else:
        text = once(text, "            Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => {\n", "            Ok(Ok(None)) | Err(mpsc::RecvTimeoutError::Disconnected) => {\n                safe_files.close();\n")
        text = once(text, "            Ok(Err(error)) => {\n", "            Ok(Err(error)) => {\n                safe_files.close();\n")
    text = once(text, "    closing: &mut bool,\n    bootstrap: &[u8],\n", "    closing: &mut bool,\n    bootstrap: &[u8],\n    safe_files: &mut SafeFiles,\n")
    text = once(text, "        && frame.payload.is_empty()\n    {\n        return Ok(());\n", "        && frame.payload.is_empty()\n    {\n        if (0x1000..=0x10ff).contains(&frame.operation) {\n            safe_files.cancel(frame.id);\n        }\n        return Ok(());\n")
    text = once(text, "            *closing = true;\n", "            *closing = true;\n            safe_files.close();\n")
    text = once(text, "        bridge::QR => bridge::encode_qr(frame).write(output),\n", "        bridge::QR => bridge::encode_qr(frame).write(output),\n        0x1000..=0x10ff => frame\n            .reply(safe_files.request(frame.id, frame.operation, &frame.payload))\n            .write(output),\n")
    return text


def managed_registration(text):
    if "McwSafeFile.WriteAllText(filePath, text, encoding);" in text and "McwSafeFile.WriteAllBytes(filePath, content);" in text:
        return text
    text = once(text, "using MagicalCryptoWallet.Helpers;", "using MagicalCryptoWallet.Mcw.Storage;")
    text = once(text, "Write(filePath, path => File.WriteAllText(path, text, encoding));", "McwSafeFile.WriteAllText(filePath, text, encoding);")
    text = once(text, "Write(filePath, path => File.WriteAllBytes(path, content));", "McwSafeFile.WriteAllBytes(filePath, content);")
    legacy = """\t\tprivate static void Write(string filePath, Action<string> write)
\t\t{
\t\t\tvar newFilePath = filePath + ".new";
\t\t\tvar oldFilePath = filePath + ".old";
\t\t\tIoHelpers.EnsureContainingDirectoryExists(newFilePath);

\t\t\twrite(newFilePath);
\t\t\tif (File.Exists(filePath))
\t\t\t{
\t\t\t\tif (File.Exists(oldFilePath))
\t\t\t\t{
\t\t\t\t\tFile.Delete(oldFilePath);
\t\t\t\t}

\t\t\t\tFile.Move(filePath, oldFilePath);
\t\t\t}

\t\t\tFile.Move(newFilePath, filePath);

\t\t\tif (File.Exists(oldFilePath))
\t\t\t{
\t\t\t\tFile.Delete(oldFilePath);
\t\t\t}
\t\t}

"""
    return once(text, legacy, "")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=pathlib.Path, required=True)
    parser.add_argument("--patch", type=pathlib.Path)
    parser.add_argument("--overlay", type=pathlib.Path)
    parser.add_argument("--managed-candidate", type=pathlib.Path)
    args = parser.parse_args()
    repo = args.repo.resolve()
    paths = ["mcw/src/app.rs", "mcw/src/lib.rs", "mcw/src/platform.rs", "MagicalCryptoWallet/Io/SafeFile.cs"]
    before = {p: subprocess.check_output(["git", "show", "HEAD:" + p], cwd=repo).decode("utf-8") for p in paths}
    after = {p: (repo / p).read_text(encoding="utf-8") for p in paths}
    after[paths[0]] = app_registration(after[paths[0]])
    after[paths[3]] = managed_registration(after[paths[3]])
    if "pub mod safe_file_service;" not in after[paths[1]]:
        previous = "pub mod round_hash;\n" if "pub mod round_hash;\n" in after[paths[1]] else "pub mod qr;\n"
        after[paths[1]] = once(after[paths[1]], previous, previous + "pub mod safe_file_service;\n")
    if "pub mod safe_file;" not in after[paths[2]]:
        after[paths[2]] += "\npub mod safe_file;\n"
    if args.patch:
        patch = "".join("".join(difflib.unified_diff(before[p].splitlines(True), after[p].splitlines(True), fromfile="a/" + p, tofile="b/" + p)) for p in paths)
        args.patch.parent.mkdir(parents=True, exist_ok=True)
        args.patch.write_text(patch, encoding="utf-8", newline="\n")
    if args.managed_candidate:
        args.managed_candidate.parent.mkdir(parents=True, exist_ok=True)
        args.managed_candidate.write_text(after[paths[3]], encoding="utf-8", newline="\n")
    if args.overlay:
        overlay = args.overlay.resolve()
        # Preserve normal Rust child-module resolution instead of assigning
        # absolute #[path] values to non-mod.rs modules such as qr/tables.rs.
        shutil.copytree(repo / "mcw/src", overlay, dirs_exist_ok=True)
        test_module = '\n#[cfg(test)]\n#[path = "' + (repo / "mcw/tests/safe_file/safe_file_host_tests.rs").as_posix() + '"]\nmod safe_file_host_registration_tests;\n'
        (overlay / "app.rs").write_text(after[paths[0]] + test_module, encoding="utf-8")
        (overlay / "platform.rs").write_text(after[paths[2]], encoding="utf-8")
        (overlay / "lib.rs").write_text(after[paths[1]], encoding="utf-8")


if __name__ == "__main__":
    main()
