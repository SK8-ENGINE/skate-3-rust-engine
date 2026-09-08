@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\native-trainer\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\native-trainer\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\native-trainer\skate3-native-trainer.exe" (
  echo Build missing. Run Build-NativeTrainer.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MODS=%~dp0mods"
set "SKATE3_MOD_SETTINGS=%~dp0.local\native-trainer\settings"
if exist "%~dp0.local\native-trainer\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\native-trainer\overlay.path"
echo Open Mods and ENABLE Native Trainer. Escape shows its settings on the left.
echo Trainer log: %~dp0.local\native-trainer\session.log
"%~dp0.local\native-trainer\skate3-native-trainer.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\native-trainer\session.log" 2>&1
if errorlevel 1 pause
