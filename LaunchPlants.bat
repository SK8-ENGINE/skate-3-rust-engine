@echo off
setlocal
cd /d "%~dp0"
set "PLANT_EXE=%~dp0bin\plants\skate3rust.exe"
set "PLANT_ASSETS=C:\Users\Daddy\AppData\Local\Skate3RustEngine\installations\957f4f78db9b4f59959f10dff8ff6a84\assets"
if not exist "%PLANT_EXE%" (
    echo Missing task build: "%PLANT_EXE%"
    pause
    exit /b 1
)
if not exist "%~dp0logs" mkdir "%~dp0logs"
set "PLANT_STAMP="
for /f %%T in ('powershell.exe -NoProfile -Command "Get-Date -Format yyyyMMdd-HHmmss-fff"') do set "PLANT_STAMP=%%T"
if not defined PLANT_STAMP set "PLANT_STAMP=run"
set "PLANT_LOG=%~dp0logs\plants-%PLANT_STAMP%-%RANDOM%.log"
> "%PLANT_LOG%" echo Plants run: %DATE% %TIME%
if errorlevel 1 (
    echo Cannot write crash log: "%PLANT_LOG%"
    pause
    exit /b 1
)
if exist "%~dp0bin\plants\BUILD.txt" type "%~dp0bin\plants\BUILD.txt" >> "%PLANT_LOG%"
set "RUST_BACKTRACE=full"
set "RUST_LOG_STYLE=never"
set "NO_COLOR=1"
echo Saving game output to "%PLANT_LOG%"
"%PLANT_EXE%" --assets "%PLANT_ASSETS%" --test-world %* >> "%PLANT_LOG%" 2>&1
set "PLANT_EXIT=%ERRORLEVEL%"
>> "%PLANT_LOG%" echo Exit code: %PLANT_EXIT%
if not "%PLANT_EXIT%"=="0" (
    type "%PLANT_LOG%"
    echo.
    echo Game exited with code %PLANT_EXIT%. Log saved to "%PLANT_LOG%"
    pause
)
exit /b %PLANT_EXIT%
