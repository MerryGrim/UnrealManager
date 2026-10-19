@echo off
setlocal
cd /d "%~dp0source"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0source\BUILD-WINDOWS.ps1" -Configuration both
set "UM_RESULT=%ERRORLEVEL%"
if "%UM_RESULT%"=="0" powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0source\PACKAGE-PORTABLE.ps1"
if "%UM_RESULT%"=="0" set "UM_RESULT=%ERRORLEVEL%"
if not "%UM_RESULT%"=="0" echo Build failed. See the error above.
pause
exit /b %UM_RESULT%
