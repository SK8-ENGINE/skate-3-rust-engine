"""First-run setup window and packaged Python conversion worker."""
from pathlib import Path
import argparse,os,queue,runpy,sys,threading,traceback

ROOT=Path(getattr(sys,'_MEIPASS',Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(ROOT))

def saved_source(marker):
    path=marker.get('source')
    if not isinstance(path,str):return None
    selected=Path(path)
    if selected.is_file() or selected.is_dir():return selected
    return None

class TolerantStream:
    """Console stream whose write/flush failures are ignored.

    Progress output is diagnostics only. The setup exe inherits the game's
    console pipe, and when that pipe or console is gone a print raises
    OSError (Errno 22 / broken pipe) and aborted the whole conversion (#20).
    After the first failure output is dropped; files and logs are unaffected."""
    def __init__(self,stream):
        self._stream=stream;self._dead=False
    def write(self,text):
        if not self._dead:
            try:return self._stream.write(text)
            except (OSError,ValueError):self._dead=True
        return len(text)
    def writelines(self,lines):
        for line in lines:self.write(line)
    def flush(self):
        if not self._dead:
            try:self._stream.flush()
            except (OSError,ValueError):self._dead=True
    def __getattr__(self,name):
        return getattr(self._stream,name)

def tolerate_dead_console():
    for name in ('stdout','stderr'):
        stream=getattr(sys,name)
        # A windowed exe has no console stream at all; print() already skips None.
        if stream is not None and not isinstance(stream,TolerantStream):setattr(sys,name,TolerantStream(stream))

LONG_PATHS_KEY=r'SYSTEM\CurrentControlSet\Control\FileSystem'

def long_paths_enabled():
    import winreg
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,LONG_PATHS_KEY) as key:
            return winreg.QueryValueEx(key,'LongPathsEnabled')[0]==1
    except OSError:return False

def enable_long_paths():
    """Ask for elevation once to enable Win32 long paths; True only if this call enabled them."""
    # Character roster work paths below data/installations exceed MAX_PATH for
    # installs in deep folders. Declining UAC keeps setup running; only the
    # affected optional characters are then reported unavailable.
    if os.name!='nt' or long_paths_enabled():return False
    import subprocess
    command=("Start-Process reg.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList "
             f"'add','HKLM\\{LONG_PATHS_KEY}','/v','LongPathsEnabled','/t','REG_DWORD','/d','1','/f'")
    subprocess.run(['powershell.exe','-NoProfile','-NonInteractive','-Command',command],
                   creationflags=subprocess.CREATE_NO_WINDOW)
    return long_paths_enabled()

def restart():
    # Windows reads LongPathsEnabled once at process start.
    import subprocess
    env=os.environ.copy()
    if getattr(sys,'frozen',False):
        env['PYINSTALLER_RESET_ENVIRONMENT']='1'
        command=[sys.executable,*sys.argv[1:]]
    else:command=[sys.executable,str(Path(__file__).resolve()),*sys.argv[1:]]
    return subprocess.call(command,env=env)

def ensure_dependencies():
    """Source checkout: setup brings its own packages. When numpy / Pillow are missing from this Python (the game
    starts `py -3.13 tools/setup.py`, a system Python), create or update <checkout>/.local/venv-setup from
    tools/requirements-setup.txt (the environment tools/setup-dev-assets.ps1 uses too) and run setup again with it.
    Returns an exit code when setup ran (or failed) in the other Python, None to go on here. The packaged setup exe
    carries its packages."""
    if getattr(sys,'frozen',False):return None
    try:
        import numpy,PIL  # noqa: F401
        return None
    except ImportError:pass
    import subprocess
    venv=ROOT/'.local'/'venv-setup'
    python=venv/'Scripts'/'python.exe' if os.name=='nt' else venv/'bin'/'python'
    inside=Path(sys.prefix).resolve()==venv.resolve()
    requirements=[line.strip() for line in (ROOT/'tools'/'requirements-setup.txt').read_text(encoding='utf-8').splitlines()
                  if line.strip() and not line.strip().startswith(('#','pyinstaller'))]
    print('Setup: installing its Python packages into '+str(venv)+' (first run only, about a minute)',flush=True)
    try:
        if not python.is_file():subprocess.run([sys.executable,'-m','venv',str(venv)],check=True)
        subprocess.run([str(python),'-m','pip','install','--disable-pip-version-check','-q',*requirements],check=True)
    except (OSError,subprocess.CalledProcessError) as error:
        try:
            import tkinter as tk
            from tkinter import messagebox
            root=tk.Tk();root.withdraw()
            messagebox.showerror('Setup could not finish',f'Setup could not install its Python packages ({error}).\n\n'
                                 f'Check the internet connection, then try again. Environment: {venv}')
        except Exception:pass
        print(f'Setup could not install its Python packages: {error}',flush=True)
        return 1
    if inside:return None  # already the setup environment: the packages are there now
    return subprocess.call([str(python),str(Path(__file__).resolve()),*sys.argv[1:]])

def headless(args):
    from tools.asset_pipeline.customiser_setup import install
    from tools.asset_pipeline.versions import installed
    from tools.asset_pipeline.validation_report import summary
    refresh=args.refresh or installed(args.base) is not None
    try:
        installed_root=install(args.source,args.base,args.game_exe,lambda text:print(text,flush=True),refresh=refresh)
    except Exception:
        args.base.mkdir(parents=True,exist_ok=True)
        (args.base/'setup-error.log').write_text(traceback.format_exc(),encoding='utf-8')
        traceback.print_exc()
        print('Setup could not finish. Details: '+str(args.base/'setup-error.log'))
        return 2
    warnings=summary(installed_root)
    print(f'Ready with {len(warnings)} warnings or unavailable components. Details: {installed_root/"setup-report.json"}'
          if warnings else 'Ready')
    return 0

def main():
    tolerate_dead_console()
    code=ensure_dependencies()
    if code is not None:return code
    if len(sys.argv)>1 and sys.argv[1]=='--character-import':
        # Keep the importer inside the already versioned setup payload: even
        # protocol-1 updaters deliver it atomically with the game executable.
        importer=ROOT/'tools/mixamo_to_skate'
        sys.path.insert(0,str(importer))
        from main import main as import_character
        return import_character(sys.argv[2:])
    if len(sys.argv)>2 and sys.argv[1]=='--task':
        script=Path(sys.argv[2])
        if not script.is_absolute():script=ROOT/script
        script=script.resolve()
        if not script.is_relative_to((ROOT/'tools').resolve()):raise RuntimeError('Invalid conversion script')
        sys.path.insert(0,str(script.parent))
        sys.argv=[str(script),*sys.argv[3:]]
        runpy.run_path(str(script),run_name='__main__')
        return 0
    parser=argparse.ArgumentParser()
    parser.add_argument('--base',type=Path,required=True)
    parser.add_argument('--game-exe',type=Path,required=True)
    parser.add_argument('--refresh',action='store_true')
    parser.add_argument('--source',type=Path,help='Xbox 360 ISO or default.xex; runs setup without the window')
    args=parser.parse_args()
    if enable_long_paths():return restart()
    if args.source:return headless(args)
    import tkinter as tk
    from tkinter import filedialog,messagebox,ttk
    from tools.asset_pipeline.customiser_setup import install
    from tools.asset_pipeline.versions import installed, fingerprints, changed_groups
    previous=installed(args.base) if args.refresh else None
    # Only the cheap version check before the window opens: install() checks
    # the installed files (damaged(): size and time of every output, hashing only changed ones) on the worker thread
    # with progress. Doing it here too kept the window closed for minutes.
    changed=changed_groups(previous[1].get('pipelines',{}),fingerprints()) if previous else set()
    updating=previous is not None
    reuse=saved_source(previous[1]) if previous else None
    window=tk.Tk()
    window.title('Skate 3 Rust Engine setup')
    window.geometry('700x320');window.resizable(False,False)
    icon=ROOT/'docs/images/skating-crab.ico'
    # Tk accepts .ico window icons only on Windows.
    if os.name=='nt' and icon.is_file():window.iconbitmap(str(icon))
    frame=ttk.Frame(window,padding=24);frame.pack(fill='both',expand=True)
    style=ttk.Style(window)
    style.configure('Note.TLabel',foreground='#666666',font=('Segoe UI',10))
    style.configure('Status.TLabel',font=('Segoe UI',10))
    style.configure('Setup.Horizontal.TProgressbar',thickness=8)
    ttk.Label(frame,text='Updating game files' if updating else 'Set up Skate 3 Rust Engine',font=('Segoe UI',20)).pack(anchor='w',pady=(0,12))
    if updating:
        names={'hud':'HUD'}
        parts=[names.get(g,g) for g in sorted(changed)]
        lead=('Converting what changed: '+', '.join(parts)+'.') if parts else 'Checking your game files.'
        note='Your current files stay in place until this finishes.'
    else:
        lead='Pick your Skate 3 Xbox 360 ISO, or default.xex in an extracted game folder.'
        note='The first conversion takes a while. ISO extraction needs an internet connection.'
    ttk.Label(frame,text=lead,font=('Segoe UI',11),wraplength=640,justify='left').pack(anchor='w')
    ttk.Label(frame,text=note,style='Note.TLabel',wraplength=640,justify='left').pack(anchor='w',pady=(4,0))
    status=tk.StringVar(value='Starting…' if updating and reuse else 'Choose your game to begin.')
    ttk.Label(frame,textvariable=status,style='Status.TLabel',wraplength=640).pack(anchor='w',side='top',pady=(36,6))
    progress=ttk.Progressbar(frame,mode='indeterminate',style='Setup.Horizontal.TProgressbar');progress.pack(fill='x')
    messages=queue.Queue();running=False;success=False
    def begin(iso):
        nonlocal running
        button.config(state='disabled');running=True;progress.start()
        def work():
            try:
                say=lambda text:messages.put(('progress',text))
                if updating and Path(iso).suffix.lower()=='.iso':
                    # Nothing to rebuild: no whole-disc extraction (doc 08 "ISO refresh without extracting the disc").
                    from tools.asset_pipeline.iso_refresh import install as iso_refresh
                    installed_root=iso_refresh(Path(iso),args.base,args.game_exe,say)
                else:installed_root=install(Path(iso),args.base,args.game_exe,say,refresh=updating)
                from tools.asset_pipeline.validation_report import summary
                warnings=summary(installed_root)
                messages.put(('done',f'Ready with {len(warnings)} warnings or unavailable components. Details: {installed_root / "setup-report.json"}' if warnings else 'Ready'))
            except Exception as error:
                args.base.mkdir(parents=True,exist_ok=True)
                (args.base/'setup-error.log').write_text(traceback.format_exc(),encoding='utf-8')
                # "Could not reuse the source" only when the source itself is the problem, not for every failure.
                source_failed=not (Path(iso).is_file() or Path(iso).is_dir()) or isinstance(error,(FileNotFoundError,PermissionError))
                messages.put(('error-source' if source_failed else 'error',str(error)))
        threading.Thread(target=work,daemon=True).start()
    def start():
        nonlocal running
        if running:return
        iso=filedialog.askopenfilename(parent=window,title='Select your Skate 3 default.xex or Xbox 360 ISO',
            filetypes=[('Skate 3 game','default.xex *.iso'),('Skate 3 executable','default.xex'),('Xbox 360 ISO','*.iso')])
        if not iso:return
        begin(iso)
    def close():
        if running:
            messagebox.showinfo('Setup running','Wait for the current conversion to finish. Your source game files are not modified.',parent=window)
        else:window.destroy()
    buttons=ttk.Frame(frame);buttons.pack(anchor='e',side='bottom')
    button=ttk.Button(buttons,text='Select ISO or default.xex',command=start)
    # Only when a file has to be picked: an update that reuses the recorded source starts on its own.
    def show_button():
        if not button.winfo_ismapped():button.pack(side='left')
    if not (updating and reuse):show_button()
    def poll():
        nonlocal running,success
        while not messages.empty():
            kind,text=messages.get_nowait();status.set(text)
            if kind=='done':
                if text!='Ready':messagebox.showwarning('Setup completed with warnings',text,parent=window)
                running=False;success=True;progress.stop();window.destroy();return
            if kind in ('error','error-source'):
                running=False;progress.stop();button.config(state='normal');show_button()
                if reuse and text and kind=='error-source':
                    status.set('Could not reuse the previous Xbox source. Select default.xex or an ISO.')
                messagebox.showerror('Setup could not finish',text+'\n\nDetails: '+str(args.base/'setup-error.log'),parent=window)
        window.after(100,poll)
    window.protocol('WM_DELETE_WINDOW',close)
    window.after(100,poll)
    # Asset refreshes reuse the recorded disc path when it still exists, so
    # program updates do not force another ISO picker for matching installs.
    if updating and reuse is not None:
        window.after(200,lambda:begin(reuse) if not running else None)
    window.mainloop()
    return 0 if success else 2


if __name__=='__main__':raise SystemExit(main())
