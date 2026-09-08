@echo off
setlocal
cd /d "%~dp0"
set "MARKER_DIR=%~dp0.local\session-marker"
if not exist "%MARKER_DIR%\skate3-session-marker.exe" (
    echo Missing private session-marker build. See docs\session-markers.md.
    pause
    exit /b 1
)
if not defined SKATE3_ASSETS (
    if exist "%MARKER_DIR%\assets.path" set /p "SKATE3_ASSETS="<"%MARKER_DIR%\assets.path"
)
if not defined SKATE3_ASSETS (
    echo Set SKATE3_ASSETS to your extracted assets directory.
    pause
    exit /b 1
)
set "SKATE3_SESSION_MARKER_OVERLAY=%MARKER_DIR%\overlay"
"%MARKER_DIR%\skate3-session-marker.exe" --assets "%SKATE3_ASSETS%" --start-paused %*
if errorlevel 1 pause
