@echo off
setlocal
cd /d "%~dp0"
if not defined SKATE3_ASSETS (
  if exist "%~dp0.local\lua-sdk\assets.path" set /p "SKATE3_ASSETS="<"%~dp0.local\lua-sdk\assets.path"
)
if not defined SKATE3_ASSETS (
  echo Set SKATE3_ASSETS to your prepared assets directory.
  pause
  exit /b 1
)
if not exist "%~dp0.local\lua-sdk\skate3-lua-sdk.exe" (
  echo Build missing. Run Build-LuaSDK.ps1 first.
  pause
  exit /b 1
)
set "SKATE3_MODS=%~dp0mods"
set "SKATE3_MOD_SETTINGS=%~dp0.local\lua-sdk\settings"
if exist "%~dp0.local\lua-sdk\overlay.path" set /p "SKATE3_SESSION_MARKER_OVERLAY="<"%~dp0.local\lua-sdk\overlay.path"
echo Open Mods from the pause menu. Examples start disabled.
echo SDK log: %~dp0.local\lua-sdk\session.log
"%~dp0.local\lua-sdk\skate3-lua-sdk.exe" --assets "%SKATE3_ASSETS%" --start-paused %* > "%~dp0.local\lua-sdk\session.log" 2>&1
if errorlevel 1 pause
