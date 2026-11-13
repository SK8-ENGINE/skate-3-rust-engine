@echo off
rem Second PC: play with a session log, then send the logs to the main PC's shared folder.
rem Put this file and play-and-send.ps1 next to skate3rust.exe and double-click this one.
rem   PLAY-AND-SEND.bat send    sends pending logs without playing
rem   PLAY-AND-SEND.bat setup   asks for the main PC and password again
cd /d "%~dp0"
set "ARGS="
if /i "%~1"=="send" set "ARGS=-SendOnly"
if /i "%~1"=="setup" set "ARGS=-Setup"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-and-send.ps1" %ARGS%
pause
