@echo off
setlocal
cd /d "%~dp0"
if not exist "%~dp0skate3rust-customiser-v4.exe" (
  echo The V4 customiser executable is missing.
  pause
  exit /b 1
)
if not exist "%~dp0assets\private\customisation\library-v3.json" (
  echo The prepared character library is missing.
  pause
  exit /b 1
)
echo Skate 3 Character Customiser V4
echo Open Escape / Start, then Character customiser.
echo Browse with arrows / D-pad. Change with left-right or the minus-plus buttons.
echo Right stick rotates your character.
echo Type to search an item list. Escape / B goes back. Done saves and resumes.
echo Diagnostic output: customiser-v4-session.log
"%~dp0skate3rust-customiser-v4.exe" --assets "%~dp0assets" --map "%~dp0maps\SkateSchool.skate" > "%~dp0customiser-v4-session.log" 2>&1
set "skate_customiser_exit=%errorlevel%"
if not "%skate_customiser_exit%"=="0" (
  echo The test exited with code %skate_customiser_exit%.
  echo See %~dp0customiser-v4-session.log
  pause
)
exit /b %skate_customiser_exit%
