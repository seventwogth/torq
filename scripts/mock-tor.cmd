@echo off
setlocal EnableDelayedExpansion

set "LOG_PATH=%~dp0mock-tor.log"

:parse
if "%~1"=="" goto run
if /I "%~1"=="--Log" (
    call :extract_log_path "%~2"
    shift
    shift
    goto parse
)
shift
goto parse

:extract_log_path
set "LOG_SPEC=%~1"
for /f "tokens=1,2,*" %%A in ("%LOG_SPEC%") do (
    if /I "%%B"=="file" set "LOG_PATH=%%C"
)
exit /b 0

:run
echo Bootstrapped 5%%>"%LOG_PATH%"
timeout /t 1 /nobreak >nul
echo Bootstrapped 45%%>>"%LOG_PATH%"
timeout /t 1 /nobreak >nul
echo Bootstrapped 100%%>>"%LOG_PATH%"

:wait_forever
timeout /t 30 /nobreak >nul
goto wait_forever
