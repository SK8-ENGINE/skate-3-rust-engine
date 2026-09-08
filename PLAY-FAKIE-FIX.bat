@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\fakie-fix\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\fakie-fix\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\fakie-fix\skate3-fakie-fix.exe" (
  echo Build missing. Run Build-FakieFix.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MOD_SETTINGS=%~dp0.local\fakie-fix\settings"
if exist "%~dp0.local\fakie-fix\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\fakie-fix\overlay.path"
echo Open Mods and ENABLE Native Trainer. Escape shows draggable settings windows. Drag the title to move, drag // to resize, or scroll for more settings.
echo Enable Hold fakie stance in the trainer settings to stop automatic fakie correction.
echo Mods folder: %~dp0.local\fakie-fix\mods
echo Trainer log: %~dp0.local\fakie-fix\session.log
"%~dp0.local\fakie-fix\skate3-fakie-fix.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\fakie-fix\session.log" 2>&1
if errorlevel 1 pause
