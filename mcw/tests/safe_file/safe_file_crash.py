"""Process-interruption tests on synthetic files; physical power-loss is unverified."""
import argparse,json,pathlib,subprocess

def select(path):
    old=pathlib.Path(str(path)+'.old');new=pathlib.Path(str(path)+'.new')
    if path.is_file() and new.is_file():return path.read_bytes()
    if old.is_file() and new.is_file():return old.read_bytes()
    if path.is_file():return path.read_bytes()
    return None

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--driver',type=pathlib.Path,required=True);parser.add_argument('--output',type=pathlib.Path,required=True);a=parser.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    cases=[]
    for boundary in ['ParentReady','NewOpened','ChunkWritten','NewSynced','PreviousBackupRemoved','OriginalBackedUp','NewInstalled','BackupRemoved','Completed']:
        folder=a.output/boundary;folder.mkdir();path=folder/'synthetic.wallet';path.write_bytes(b'old-complete');pathlib.Path(str(path)+'.old').write_bytes(b'previous-backup')
        p=subprocess.run([str(a.driver),'crash',str(path),boundary],capture_output=True,timeout=15)
        assert p.returncode==71,(boundary,p.returncode,p.stderr)
        found=select(path);assert found in [b'old-complete',b'new-complete'],(boundary,found)
        cases.append({'boundary':boundary,'exit_code':p.returncode,'recovered':'old' if found==b'old-complete' else 'new'})
    summary={'passed':True,'cases':cases,'role':'synthetic owned process interruption; not physical power-loss certification'}
    (a.output/'crash.json').write_text(json.dumps(summary,indent=2));print(json.dumps({'passed':True,'interruption_cases':len(cases)}))
if __name__=='__main__':main()
