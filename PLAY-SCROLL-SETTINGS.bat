@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\scroll-settings\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\scroll-settings\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\scroll-settings\skate3-scroll-settings.exe" (
  echo Build missing. Run Build-ScrollSettings.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MOD_SETTINGS=%~dp0.local\scroll-settings\settings"
if exist "%~dp0.local\scroll-settings\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\scroll-settings\overlay.path"
echo Open Mods and ENABLE Native Trainer. Escape shows draggable settings windows. Drag their title bars; scroll the mouse wheel for more settings.
echo Mods folder: %~dp0.local\scroll-settings\mods
echo Trainer log: %~dp0.local\scroll-settings\session.log
"%~dp0.local\scroll-settings\skate3-scroll-settings.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\scroll-settings\session.log" 2>&1
if errorlevel 1 pause
