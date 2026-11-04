@echo off
rem Gets SkateLauncher.exe next to this file: the release's prebuilt one when it was built from this SkateLauncher.cs,
rem otherwise built here with the C# compiler that ships with Windows (.NET Framework 4). Then starts it.
cd /d "%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0launcher.ps1" launcher-exe >nul 2>&1
if not exist SkateLauncher.exe "%WINDIR%\Microsoft.NET\Framework64\v4.0.30319\csc.exe" /nologo /target:winexe /out:SkateLauncher.exe SkateLauncher.cs
if not exist SkateLauncher.exe (echo Could not get SkateLauncher.exe. & pause & exit /b 1)
echo Ready: %~dp0SkateLauncher.exe
if /i not "%~1"=="/nostart" start "" "%~dp0SkateLauncher.exe"
