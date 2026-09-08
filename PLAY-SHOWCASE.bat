@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\showcase\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\showcase\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\showcase\skate3-showcase.exe" (
  echo Build missing. Run Build-Showcase.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MOD_SETTINGS=%~dp0.local\showcase\settings"
if exist "%~dp0.local\showcase\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\showcase\overlay.path"
echo Open Mods and ENABLE Native Trainer. Escape shows draggable settings windows. Drag their title bars.
echo Mods folder: %~dp0.local\showcase\mods
echo Trainer log: %~dp0.local\showcase\session.log
"%~dp0.local\showcase\skate3-showcase.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\showcase\session.log" 2>&1
if errorlevel 1 pause
