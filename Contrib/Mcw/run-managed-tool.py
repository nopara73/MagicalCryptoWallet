#!/usr/bin/env python3
"""Run an inert managed verification tool through the production mcw host."""
import argparse, os, shutil, subprocess, tempfile
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument("--host",required=True,type=Path)
parser.add_argument("--project",required=True,type=Path)
parser.add_argument("arguments",nargs=argparse.REMAINDER)
args=parser.parse_args()
project=args.project.resolve()
subprocess.run(["dotnet","build",str(project),"-c","Release"],check=True)
source=project / "bin/Release/net10.0"
suffix=".exe" if os.name=="nt" else ""
tools=list(project.glob("*.csproj"))
if len(tools)!=1:raise RuntimeError("Expected one verification tool project")
name=tools[0].stem
arguments=args.arguments[1:] if args.arguments[:1]==["--"] else args.arguments
with tempfile.TemporaryDirectory(prefix="mcw verification tool ",dir=ROOT/".artifacts") as directory:
    work=Path(directory);shutil.copytree(source,work,dirs_exist_ok=True)
    shutil.copy2(args.host.resolve(),work/("mcw"+suffix))
    shutil.copy2(work/(name+suffix),work/("magicalcryptowallet"+suffix))
    # Explicit handles also capture diagnostics from Windows GUI-subsystem apps.
    result = subprocess.run([str(work/("mcw"+suffix)),"gui",*arguments],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    print(result.stdout, end="")
    result.check_returncode()
