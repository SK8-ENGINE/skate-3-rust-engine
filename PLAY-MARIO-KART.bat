@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\vehicle-sdk\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\vehicle-sdk\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\vehicle-sdk\skate3-vehicle-sdk.exe" (
  echo Build missing. Run Build-VehicleSDK.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MOD_SETTINGS=%~dp0.local\vehicle-sdk\settings"
if exist "%~dp0.local\vehicle-sdk\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\vehicle-sdk\overlay.path"
echo Open Mods and ENABLE Mario Kart. F10 spawns the kart; E enters/exits. Escape shows draggable settings windows. Drag the title to move, drag // to resize, or scroll for more settings.
echo WASD drive, Space brake, Shift handbrake, R reset. Slow down before exiting.
echo Mods folder: %~dp0.local\vehicle-sdk\mods
echo Trainer log: %~dp0.local\vehicle-sdk\session.log
"%~dp0.local\vehicle-sdk\skate3-vehicle-sdk.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\vehicle-sdk\session.log" 2>&1
if errorlevel 1 pause
