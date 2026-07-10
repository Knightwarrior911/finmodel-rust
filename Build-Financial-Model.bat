@echo off
setlocal enabledelayedexpansion
REM finmodel - double-click launcher (demo). No commands, no toolchain needed.
REM Anchors to this file's folder so it works from Explorer double-click.
cd /d "%~dp0"

set "EXE=finmodel-core\target\release\fm-cli.exe"

if not exist "%EXE%" (
    echo.
    echo ERROR: the finmodel engine was not found at:
    echo   %~dp0%EXE%
    echo Ask your developer to build it once, then re-run this.
    echo.
    pause
    exit /b 1
)

echo ============================================
echo    finmodel  -  Financial Model Builder
echo ============================================
echo.
echo   Type a company ticker and press Enter.
echo.
echo   Demo companies (real data, works offline):
echo     SAND.ST     Sandvik        (Sweden)
echo     ASML.AS     ASML           (Netherlands)
echo     NOVO-B.CO   Novo Nordisk   (Denmark)
echo     NESN.SW     Nestle         (Switzerland)
echo     ATCO-B.ST   Atlas Copco    (Sweden)
echo.
set "TICKER="
set /p "TICKER=Ticker (press Enter for SAND.ST): "
if "!TICKER!"=="" set "TICKER=SAND.ST"

echo.
echo Building the model for !TICKER! ... please wait.
echo.
"%EXE%" build !TICKER!
if errorlevel 1 (
    echo.
    echo ---------------------------------------------
    echo  Build did not complete. See the message above.
    echo ---------------------------------------------
    pause
    exit /b 1
)

REM Output filename: ticker with . and / replaced by _
set "STEM=!TICKER:.=_!"
set "STEM=!STEM:/=_!"
set "XLSX=!STEM!_model.xlsx"

if exist "!XLSX!" (
    echo.
    echo  Done. Opening the Excel model: !XLSX!
    start "" "!XLSX!"
    echo.
    echo  You can close this window.
    timeout /t 5 >nul
) else (
    echo.
    echo  WARNING: expected output "!XLSX!" was not found.
    pause
)
endlocal
