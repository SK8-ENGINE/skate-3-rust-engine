@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\launch-multiplayer-test.ps1" -BuildDirectory bin/multiplayer-lobbies -MenuOnly %*
if errorlevel 1 pause
