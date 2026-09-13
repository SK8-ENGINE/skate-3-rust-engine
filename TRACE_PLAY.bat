@echo off
cd /d "%~dp0"
set "SKATE_TRACE_FILE=trace-downtown-lag.json"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Launch.ps1" -Trace
if errorlevel 1 pause
