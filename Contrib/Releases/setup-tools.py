#!/usr/bin/env python3
"""Download checksum-pinned platform packaging tools into the build workspace."""
import argparse, hashlib, os, sys, urllib.request, zipfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def fetch(url, destination, expected):
    if not destination.exists():
        with urllib.request.urlopen(url) as source, destination.open('wb') as output:
            while chunk:=source.read(1024*1024): output.write(chunk)
    if hashlib.sha256(destination.read_bytes()).hexdigest()!=expected: raise RuntimeError('Tool checksum mismatch: '+destination.name)
    return destination

parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--rid',required=True);args=parser.parse_args()
tools=ROOT/'.artifacts/tools';tools.mkdir(parents=True,exist_ok=True)
import subprocess
subprocess.run([sys.executable, str(ROOT/'Contrib/Mcw/setup-rust.py'), '--rid', args.rid], check=True)
if args.rid.startswith('win'):
    downloads=[
      ('https://github.com/brechtsanders/winlibs_mingw/releases/download/16.2.0posix-14.0.0-ucrt-r2/winlibs-x86_64-posix-seh-gcc-16.2.0-mingw-w64ucrt-14.0.0-r2.zip','mingw.zip','d5dbafc4a170e762ca6143151ec918fb9e2c72736fb14cd704abebc6bdd5276a','.'),
      ('https://github.com/Kitware/CMake/releases/download/v3.31.8/cmake-3.31.8-windows-x86_64.zip','cmake.zip','81aa9964dbabd71fe02e7ec50472fd3ad56138c49944515ece9001efbff8d719','.'),
      ('https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip','wix.zip','6ac824e1642d6f7277d0ed7ea09411a508f6116ba6fae0aa5f2c7daa2ff43d31','wix314')]
    for url,name,expected,folder in downloads:
        archive=fetch(url,tools/name,expected)
        with zipfile.ZipFile(archive) as contents: contents.extractall(tools/folder)
    paths=[tools/'mingw64/bin',tools/'cmake-3.31.8-windows-x86_64/bin',tools/'wix314']
    if path_file:=os.environ.get('GITHUB_PATH'):
        with open(path_file,'a') as stream:
            stream.write('\n'.join(str(p) for p in paths)+'\n')
    else: print('Add these directories to PATH: '+os.pathsep.join(str(p) for p in paths))
elif args.rid.startswith('linux'):
    arch='aarch64' if args.rid.endswith('arm64') else 'x86_64'
    expected='1b00524ba8c6b678dc15ef88a5c25ec24def36cdfc7e3abb32ddcd068e8007fe' if arch=='aarch64' else 'a6d71e2b6cd66f8e8d16c37ad164658985e0cf5fcaa950c90a482890cb9d13e0'
    tool=fetch(f'https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-{arch}.AppImage',tools/f'appimagetool-{arch}.AppImage',expected)
    tool.chmod(0o755)
    if env_file:=os.environ.get('GITHUB_ENV'):
        with open(env_file,'a') as stream:stream.write(f'APPIMAGETOOL={tool}\nAPPIMAGE_EXTRACT_AND_RUN=1\n')
    else: print('APPIMAGETOOL='+str(tool)+' APPIMAGE_EXTRACT_AND_RUN=1')
