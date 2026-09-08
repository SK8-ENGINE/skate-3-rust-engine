@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\launch-multiplayer-test.ps1" -Players 10 %*
if errorlevel 1 pause
