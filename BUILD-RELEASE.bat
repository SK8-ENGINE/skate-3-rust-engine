@echo off
setlocal
cd /d "%~dp0"
if not defined ProgramFiles(x86) set "ProgramFiles(x86)=C:\Program Files (x86)"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Build-Release.ps1"
if errorlevel 1 pause
