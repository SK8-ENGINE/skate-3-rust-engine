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
"%PLANT_EXE%" --assets "%PLANT_ASSETS%" --test-world %*
set "PLANT_EXIT=%ERRORLEVEL%"
if not "%PLANT_EXIT%"=="0" pause
exit /b %PLANT_EXIT%
