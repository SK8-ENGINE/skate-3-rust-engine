"""The project's look for the tkinter windows (setup, updater): the startup splash's palette (crates/skate-game/src/splash.rs)
so a refresh that needs a window looks like the splash it came from. Ask 611, doc 08 "Setup windows".
"""

BACKGROUND = '#0e0e11'
PANEL = '#18181c'
GRIP = '#222226'
TITLE = '#a0a0a5'
TEXT = '#d9d9d9'
DIM = '#6e6e73'
PLY = '#d6b07c'
PLY_DARK = '#96683e'
WARNING = '#ecac4c'
FONT = 'Segoe UI'


def apply(window):
    """Dark ttk styles on `window`; returns the ttk.Style. Named styles: Title.TLabel, Lead.TLabel, Note.TLabel,
    Status.TLabel, Warning.TLabel, Accent.TButton, Setup.Horizontal.TProgressbar."""
    from tkinter import ttk
    window.configure(background=BACKGROUND)
    style = ttk.Style(window)
    style.theme_use('clam')
    style.configure('.', background=BACKGROUND, foreground=TEXT, font=(FONT, 10), fieldbackground=PANEL,
                    bordercolor=GRIP, lightcolor=GRIP, darkcolor=GRIP, troughcolor=GRIP, focuscolor=PLY,
                    selectbackground=PLY_DARK, selectforeground=TEXT, insertcolor=TEXT)
    style.configure('TFrame', background=BACKGROUND)
    style.configure('TLabel', background=BACKGROUND, foreground=TEXT)
    style.configure('Title.TLabel', foreground=TITLE, font=(FONT, 13, 'bold'))
    style.configure('Lead.TLabel', foreground=TEXT, font=(FONT, 11))
    style.configure('Note.TLabel', foreground=DIM, font=(FONT, 10))
    style.configure('Status.TLabel', foreground=TEXT, font=(FONT, 10))
    style.configure('Warning.TLabel', foreground=WARNING, font=(FONT, 10, 'bold'))
    style.configure('TButton', background=PANEL, foreground=TEXT, bordercolor=DIM, padding=(14, 6), font=(FONT, 10, 'bold'))
    style.map('TButton', background=[('disabled', BACKGROUND), ('active', GRIP)], foreground=[('disabled', DIM)],
              bordercolor=[('active', PLY)])
    style.configure('Accent.TButton', background=PLY_DARK, foreground=TEXT, bordercolor=PLY)
    style.map('Accent.TButton', background=[('disabled', BACKGROUND), ('active', PLY)], foreground=[('disabled', DIM), ('active', BACKGROUND)])
    style.configure('Setup.Horizontal.TProgressbar', thickness=6, background=PLY, troughcolor=GRIP, bordercolor=GRIP,
                    lightcolor=PLY, darkcolor=PLY_DARK)
    style.configure('TCombobox', fieldbackground=PANEL, background=PANEL, foreground=TEXT, arrowcolor=TEXT)
    style.map('TCombobox', fieldbackground=[('readonly', PANEL)], foreground=[('readonly', TEXT)])
    window.option_add('*TCombobox*Listbox.background', PANEL)
    window.option_add('*TCombobox*Listbox.foreground', TEXT)
    window.option_add('*TCombobox*Listbox.selectBackground', PLY_DARK)
    return style


def text_widget(widget):
    """A plain tk.Text in the same colors."""
    widget.configure(background=PANEL, foreground=TEXT, insertbackground=TEXT, relief='flat', highlightthickness=1,
                     highlightbackground=GRIP, highlightcolor=PLY_DARK, font=(FONT, 10), padx=10, pady=8)
