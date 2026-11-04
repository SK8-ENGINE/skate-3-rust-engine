@echo off
rem Get the main PC's local hails-additions (its pushed hails-additions plus unpushed work) and add it to the launcher.
rem   GET-PATCHES.bat           copies and applies the newest patch from the main PC
rem   GET-PATCHES.bat -Status   shows what the main PC shares and what is applied here
rem   GET-PATCHES.bat -Remove   removes the hails-local copy and its launcher version
rem Guide: docs/hails-additions/second-pc-testing.md#local-hails-additions-patches
setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0get-patches.ps1" %*
pause
