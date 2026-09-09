"""First-run setup window and packaged Python conversion worker."""
from pathlib import Path
import argparse,os,queue,runpy,sys,threading,traceback

ROOT=Path(getattr(sys,'_MEIPASS',Path(__file__).resolve().parents[1]))
sys.path.insert(0,str(ROOT))

def main():
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
    args=parser.parse_args()
    import tkinter as tk
    from tkinter import filedialog,messagebox,ttk
    from tools.asset_pipeline.install import install
    window=tk.Tk()
    window.title('Skate 3 Rust Engine setup')
    window.geometry('700x420');window.resizable(False,False)
    icon=ROOT/'docs/images/skating-crab.ico'
    if icon.is_file():window.iconbitmap(str(icon))
    frame=ttk.Frame(window,padding=24);frame.pack(fill='both',expand=True)
    ttk.Label(frame,text='Set up Skate 3 Rust Engine',font=('Segoe UI',20)).pack(anchor='w',pady=(0,16))
    ttk.Label(frame,text='Select your Skate 3 Xbox 360 ISO, or default.xex inside an\nextracted game folder. Keep the game data beside default.xex.\nSetup prepares the skater, animations and all disc maps.\nNo other apps need installing.\n\nISO extraction needs internet access. Allow free disk space\nand time for the first conversion.',
              font=('Segoe UI',11),justify='left').pack(anchor='w')
    status=tk.StringVar(value='Choose your game to begin.')
    ttk.Label(frame,textvariable=status,wraplength=600).pack(anchor='w',pady=(18,8))
    progress=ttk.Progressbar(frame,mode='indeterminate');progress.pack(fill='x')
    messages=queue.Queue();running=False;success=False
    def start(folder=False):
        nonlocal running
        iso=(filedialog.askdirectory(parent=window,title='Select the Skate 3 folder containing default.xex') if folder else
             filedialog.askopenfilename(parent=window,title='Select your Skate 3 Xbox 360 game',
                 filetypes=[('Xbox 360 game','*.iso *.xex'),('Xbox 360 ISO','*.iso'),('Xbox executable','*.xex')]))
        if not iso:return
        button.config(state='disabled');folder_button.config(state='disabled');running=True;progress.start()
        def work():
            try:
                install(Path(iso),args.base,args.game_exe,lambda text:messages.put(('progress',text)))
                messages.put(('done','Ready'))
            except Exception as error:
                args.base.mkdir(parents=True,exist_ok=True)
                (args.base/'setup-error.log').write_text(traceback.format_exc(),encoding='utf-8')
                messages.put(('error',str(error)))
        threading.Thread(target=work,daemon=True).start()
    def close():
        if running:
            messagebox.showinfo('Setup running','Wait for the current conversion to finish. Your source game files are not modified.',parent=window)
        else:window.destroy()
    buttons=ttk.Frame(frame);buttons.pack(anchor='e',pady=18)
    folder_button=ttk.Button(buttons,text='Select extracted folder',command=lambda:start(True));folder_button.pack(side='left',padx=(0,8))
    button=ttk.Button(buttons,text='Select ISO or default.xex',command=start);button.pack(side='left')
    def poll():
        nonlocal running,success
        while not messages.empty():
            kind,text=messages.get_nowait();status.set(text)
            if kind=='done':
                running=False;success=True;progress.stop();window.destroy();return
            if kind=='error':
                running=False;progress.stop();button.config(state='normal');folder_button.config(state='normal')
                messagebox.showerror('Setup could not finish',text+'\n\nDetails: '+str(args.base/'setup-error.log'),parent=window)
        window.after(100,poll)
    window.protocol('WM_DELETE_WINDOW',close)
    window.after(100,poll);window.mainloop()
    return 0 if success else 2

if __name__=='__main__':raise SystemExit(main())
