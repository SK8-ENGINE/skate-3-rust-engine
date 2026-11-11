@echo off
rem Custom Music: double-click to add a music folder (or drag a folder onto this file). See README.md.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0add-my-music.ps1" %1
echo.
pause
