@echo off
rem Skate launcher from a keyboard. No argument: opens the window (SkateLauncher.exe, build it with build.bat).
rem   With arguments: run an entry directly, e.g.  launcher.bat rust-play -Version upstream-main
rem   launcher.bat list  prints every entry; launcher.bat versions  lists the versions.
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0launcher.ps1" %*
