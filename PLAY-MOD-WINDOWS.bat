@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\mod-windows\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\mod-windows\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\mod-windows\skate3-mod-windows.exe" (
  echo Build missing. Run Build-ModWindows.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MODS=%~dp0mods"
set "SKATE3_MOD_SETTINGS=%~dp0.local\mod-windows\settings"
if exist "%~dp0.local\mod-windows\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\mod-windows\overlay.path"
echo Open Mods and ENABLE Native Trainer. Escape shows draggable settings windows. Drag their title bars.
echo Trainer log: %~dp0.local\mod-windows\session.log
"%~dp0.local\mod-windows\skate3-mod-windows.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\mod-windows\session.log" 2>&1
if errorlevel 1 pause
