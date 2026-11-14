@echo off
rem Play the traced Skate 3 recompilation with one trace preset, then send the session to the main PC.
rem   PLAY-RECOMP.bat            lists the presets
rem   PLAY-RECOMP.bat props      plays with the props trace, then sends
rem   PLAY-RECOMP.bat -GetGame   first time: copies the recomp game folder from the main PC
rem   PLAY-RECOMP.bat -Update    copies the newest recomp build from the main PC
rem Guide: docs/hails-additions/second-pc-testing.md#recomp-trace-sessions
setlocal
set "FIRST=%~1"
set "PS=powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0play-recomp.ps1""
if "%FIRST%"=="" (
  %PS% -List
) else if "%FIRST:~0,1%"=="-" (
  %PS% %*
) else (
  %PS% -Trace %*
)
pause
