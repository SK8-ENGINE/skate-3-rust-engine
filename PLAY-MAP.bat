@echo off
cd /d "%~dp0"
if "%~1"=="" (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Select-Map.ps1"
) else (
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Select-Map.ps1" -Map "%~1"
)
if errorlevel 1 pause
