@echo off
cd /d "%~dp0"
if /i "%~1"=="-trace" (
    if not "%~2"=="" set "SKATE_TRACE_MAP=%~2"
    if not "%~3"=="" set "SKATE_TRACE_FILE=%~3"
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Launch.ps1" -Trace
) else if "%~1"=="" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Launch.ps1"
) else (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\Launch.ps1" -Map "%~1"
)
if errorlevel 1 pause
