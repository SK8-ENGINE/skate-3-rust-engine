@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\launch-multiplayer-test.ps1" -MenuOnly %*
if errorlevel 1 pause
