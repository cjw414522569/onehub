@echo off
setlocal
cd /d "%~dp0"

set "ROOT=%CD%"
set "EXE=%ROOT%\target\debug\ssh-gui.exe"
set "DIST=%ROOT%\clients\windows\ui\dist\index.html"

echo [PC client] working dir: %ROOT%

REM 1) Build the copied mxterm UI if dist is missing.
if exist "%DIST%" goto have_dist
echo [PC client] UI dist not found, building ...
pushd "%ROOT%\clients\windows\ui"
if exist "node_modules" goto have_deps
echo [PC client] installing frontend deps (npm install) ...
call npm.cmd install --no-audit --no-fund
if errorlevel 1 goto npm_failed
:have_deps
echo [PC client] building frontend (npm run build) ...
call npm.cmd run build
if errorlevel 1 goto build_failed
popd

:have_dist
REM 2) Compile the native shell if the exe is missing.
if exist "%EXE%" goto have_exe
echo [PC client] binary not found, compiling (cargo build) ...
cargo build -p clients-windows --locked
if errorlevel 1 goto cargo_failed

:have_exe
REM 3) Launch.
echo [PC client] launching ...
start "" "%EXE%"
echo [PC client] started (window title: SSH Client - PC GUI).
goto done

:npm_failed
echo [ERROR] npm install failed
popd
goto fail

:build_failed
echo [ERROR] frontend build failed
popd
goto fail

:cargo_failed
echo [ERROR] cargo build failed
goto fail

:fail
pause
exit /b 1

:done
endlocal