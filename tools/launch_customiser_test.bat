@echo off
setlocal
cd /d "%~dp0"
if not exist "%~dp0skate3rust-customiser.exe" (
  echo The dedicated customiser executable is missing.
  pause
  exit /b 1
)
echo Skate 3 Character Customiser Test
echo Open Escape / Start, then choose Character customiser.
echo Save: F5 / controller X. Back: Escape / controller B.
echo First-time clothing changes can take a few seconds.
echo Diagnostic output is written to customiser-session.log.
"%~dp0skate3rust-customiser.exe" --assets "%~dp0assets" --map "%~dp0maps\SkateSchool.skate" > "%~dp0customiser-session.log" 2>&1
set "skate_customiser_exit=%errorlevel%"
if not "%skate_customiser_exit%"=="0" (
  echo The test exited with code %skate_customiser_exit%.
  echo See %~dp0customiser-session.log
  pause
)
exit /b %skate_customiser_exit%
