@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\fakie-trainer\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\fakie-trainer\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\fakie-trainer\skate3-fakie-trainer.exe" (
  echo Build missing. Run Build-FakieTrainer.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MOD_SETTINGS=%~dp0.local\fakie-trainer\settings"
if exist "%~dp0.local\fakie-trainer\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\fakie-trainer\overlay.path"
echo Open Mods and ENABLE Native Trainer. Escape shows draggable settings windows. Drag the title to move, drag // to resize, or scroll for more settings.
echo Enable Hold fakie stance in the trainer settings to stop automatic fakie correction.
echo Mods folder: %~dp0.local\fakie-trainer\mods
echo Trainer log: %~dp0.local\fakie-trainer\session.log
"%~dp0.local\fakie-trainer\skate3-fakie-trainer.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\fakie-trainer\session.log" 2>&1
if errorlevel 1 pause
