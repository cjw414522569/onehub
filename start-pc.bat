@echo off
setlocal
cd /d "%~dp0"

set "ROOT=%CD%"
set "UI=%ROOT%\clients\windows\ui"
set "DIST=%UI%\dist\index.html"
set "EXE=%ROOT%\target\debug\onehub.exe"

echo [OneHub PC] working dir: %ROOT%

REM 1) Install frontend deps when missing.
if exist "%UI%\node_modules" goto have_deps
echo [OneHub PC] installing frontend deps (npm install) ...
pushd "%UI%"
call npm.cmd install --no-audit --no-fund
if errorlevel 1 goto npm_failed
popd
:have_deps

REM 2) Rebuild the UI when dist is missing or older than any ui/src source.
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$dist='%DIST%'; if (-not (Test-Path $dist)) { exit 1 }; " ^
  "$newest=Get-ChildItem -Path '%UI%\src' -Recurse -File -Include *.ts,*.tsx,*.js,*.jsx,*.html,*.css -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1; " ^
  "if ($newest -and $newest.LastWriteTime -gt (Get-Item $dist).LastWriteTime) { exit 1 } else { exit 0 }"
if errorlevel 1 goto build_ui
goto have_dist

:build_ui
echo [OneHub PC] building frontend (npm run build) ...
pushd "%UI%"
call npm.cmd run build
if errorlevel 1 goto ui_build_failed
popd

:have_dist
REM 3) Always compile the native shell (incremental, fast when unchanged).
echo [OneHub PC] compiling native shell (cargo build -p clients-windows --locked) ...
cargo build -p clients-windows --locked
if errorlevel 1 goto cargo_failed

REM 4) Launch the freshly built binary.
if not exist "%EXE%" goto exe_missing
echo [OneHub PC] launching ...
start "" "%EXE%"
echo [OneHub PC] started (OneHub - PC Client).
goto done

:npm_failed
echo [ERROR] npm install failed
popd
goto fail

:ui_build_failed
echo [ERROR] frontend build failed
popd
goto fail

:cargo_failed
echo [ERROR] cargo build failed
goto fail

:exe_missing
echo [ERROR] binary not found after build: %EXE%
goto fail

:fail
pause
exit /b 1

:done
endlocal
