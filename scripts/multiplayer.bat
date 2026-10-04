@echo off
REM ---------------------------------------------------------------------------
REM Local multiplayer launcher: one session server + N Vulkan game clients.
REM
REM Run this from the folder that contains skated.exe and skate3rust.exe
REM (normally `dist/`, produced by scripts/build-multiplayer.ps1).
REM
REM All clients use --multi-instance (limited rendering, unchanged simulation).
REM
REM Usage:
REM   multiplayer.bat                 server + two Vulkan clients
REM   multiplayer.bat --instances 3   server + three Vulkan clients
REM   multiplayer.bat --test-world    use the lightweight multiplayer test scene
REM   multiplayer.bat --no-server     clients only (server already running)
REM   multiplayer.bat --dry-run       show commands without starting anything
REM ---------------------------------------------------------------------------
setlocal
set "HERE=%~dp0"

set "INSTANCES=2"
set "NO_SERVER="
set "DRY_RUN="
set "TEST_WORLD="

:parse
if "%~1"=="" goto afterparse
if /I "%~1"=="--no-server" (
    set "NO_SERVER=-NoServer"
    shift
    goto parse
)
if /I "%~1"=="--dry-run" (
    set "DRY_RUN=-DryRun"
    shift
    goto parse
)
if /I "%~1"=="--test-world" (
    set "TEST_WORLD=-TestWorld"
    shift
    goto parse
)
if /I "%~1"=="--instances" (
    if "%~2"=="" (
        echo --instances requires a number.
        exit /b 2
    )
    set "INSTANCES=%~2"
    shift
    shift
    goto parse
)
echo Unknown argument: %~1
echo Usage: multiplayer.bat [--instances N] [--test-world] [--no-server] [--dry-run]
exit /b 2
:afterparse
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%HERE%Launch-Multiplayer.ps1" -Instances "%INSTANCES%" %TEST_WORLD% %NO_SERVER% %DRY_RUN%
if errorlevel 1 (
    if not defined DRY_RUN pause
    exit /b 1
)
exit /b 0
